//! `agent-connections.json`: load, validate, save. Credentials go to the keychain.

use std::path::{Path, PathBuf};

use crate::context::{SecretKey, SecretStore};

use super::model::{
    AgentConfig, ConfigTarget, ConnectionsDocument, DOCUMENT_VERSION, MAX_FALLBACK_MODELS,
};

pub fn document_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agent-connections.json")
}

/// Absent file is an empty document; an unreadable one is an error, never
/// silently replaced (same rule as settings.json).
/// The document as routing and launching see it: plus the virtual login
/// connections that are on (`ConnectionsDocument::with_logins`). Never save it.
pub fn load_effective(path: &Path) -> Result<ConnectionsDocument, String> {
    load(path).map(ConnectionsDocument::with_logins)
}

pub fn load(path: &Path) -> Result<ConnectionsDocument, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConnectionsDocument {
            version: DOCUMENT_VERSION,
            ..Default::default()
        }),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Structural checks that would otherwise surface as confusing routing failures.
pub fn validate(doc: &ConnectionsDocument) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    for c in &doc.connections {
        if c.id.trim().is_empty() || !ids.insert(format!("c:{}", c.id)) {
            return Err(format!("duplicate or empty connection id `{}`", c.id));
        }
        if c.protocols.is_empty() {
            return Err(format!("connection `{}` lists no protocol", c.id));
        }
        let mut names = std::collections::HashSet::new();
        for m in &c.models {
            if m.upstream_id.trim().is_empty() {
                return Err(format!(
                    "connection `{}` has a model without upstream id",
                    c.id
                ));
            }
            // Two entries on one connection may not claim the same mapped name:
            // routing would have to guess which upstream model to send.
            if m.enabled && !names.insert(m.routing_name().to_string()) {
                return Err(format!(
                    "connection `{}` maps `{}` twice",
                    c.id,
                    m.routing_name()
                ));
            }
        }
    }
    let has_connection = |id: &str| doc.connections.iter().any(|c| c.id == id);
    for p in &doc.pools {
        if p.id.trim().is_empty() || !ids.insert(format!("p:{}", p.id)) {
            return Err(format!("duplicate or empty pool id `{}`", p.id));
        }
        for m in &p.members {
            if !has_connection(&m.connection_id) {
                return Err(format!(
                    "pool `{}` references missing connection `{}`",
                    p.id, m.connection_id
                ));
            }
        }
    }
    let has_pool = |id: &str| doc.pools.iter().any(|p| p.id == id);
    for cfg in &doc.configs {
        if cfg.id.trim().is_empty() || !ids.insert(format!("a:{}", cfg.id)) {
            return Err(format!("duplicate or empty config id `{}`", cfg.id));
        }
        let ok = match &cfg.target {
            ConfigTarget::Connection { connection_id } => has_connection(connection_id),
            ConfigTarget::Pool { pool_id } => has_pool(pool_id),
            ConfigTarget::Route { pool_id } => pool_id.as_deref().is_none_or(has_pool),
        };
        if !ok {
            return Err(format!(
                "config `{}` targets something that does not exist",
                cfg.id
            ));
        }
        validate_fallbacks(cfg)?;
    }
    Ok(())
}

/// R8: at most three, no blanks, no repeats, never the default model itself.
fn validate_fallbacks(cfg: &AgentConfig) -> Result<(), String> {
    if cfg.fallback_models.len() > MAX_FALLBACK_MODELS {
        return Err(format!(
            "config `{}` lists more than {MAX_FALLBACK_MODELS} fallback models",
            cfg.id
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for name in &cfg.fallback_models {
        let name = name.trim();
        if name.is_empty() || name == cfg.default_model.trim() || !seen.insert(name) {
            return Err(format!(
                "config `{}` has an empty, repeated or default fallback model `{name}`",
                cfg.id
            ));
        }
    }
    Ok(())
}

pub fn save(path: &Path, doc: &ConnectionsDocument) -> Result<(), String> {
    validate(doc)?;
    let mut doc = doc.clone();
    doc.version = DOCUMENT_VERSION;
    let raw = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    crate::settings::atomic_write_private(path, &raw)
}

/// Credentials are written separately from the document so the file never holds them.
pub fn set_credential(
    store: &dyn SecretStore,
    connection_id: &str,
    secret: &str,
) -> Result<(), String> {
    store.set(
        &SecretKey(format!("connection:{connection_id}:credential")),
        secret,
    )
}

pub fn get_credential(
    store: &dyn SecretStore,
    connection_id: &str,
) -> Result<Option<String>, String> {
    store.get(&SecretKey(format!("connection:{connection_id}:credential")))
}

/// Deleting a connection also deletes its credential (Q27: security removals are immediate).
pub fn delete_credential(store: &dyn SecretStore, connection_id: &str) -> Result<(), String> {
    store.delete(&SecretKey(format!("connection:{connection_id}:credential")))
}
