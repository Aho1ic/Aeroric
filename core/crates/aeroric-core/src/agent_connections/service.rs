//! Document operations behind the host `agentConnections.*` methods.
//!
//! Pure functions over [`ConnectionsDocument`]; the host does the locking, I/O and
//! keychain calls. Every mutation returns the new document so the caller can
//! validate-and-save it in one step.

use super::model::{
    AgentConfig, ConfigTarget, Connection, ConnectionsDocument, ModelEntry, ModelSource, Pool,
};

fn upsert<T>(items: &mut Vec<T>, item: T, id: impl Fn(&T) -> &str) {
    let key = id(&item).to_string();
    match items.iter_mut().find(|existing| id(existing) == key) {
        Some(slot) => *slot = item,
        None => items.push(item),
    }
}

/// Insert or replace a connection. A changed catalog bumps `catalog_revision`
/// so configs and tasks pinned to the old revision can tell (Q133).
pub fn put_connection(doc: &mut ConnectionsDocument, mut connection: Connection) {
    if let Some(old) = doc.connections.iter().find(|c| c.id == connection.id) {
        connection.catalog_revision = if old.models != connection.models {
            old.catalog_revision + 1
        } else {
            old.catalog_revision
        };
    }
    upsert(&mut doc.connections, connection, |c| &c.id);
}

/// Remove a connection. Pools drop it as a member; a config that targets it
/// directly blocks the removal, because silently retargeting a config would
/// change what new tasks run on.
pub fn delete_connection(doc: &mut ConnectionsDocument, id: &str) -> Result<(), String> {
    if let Some(config) = doc.configs.iter().find(
        |cfg| matches!(&cfg.target, ConfigTarget::Connection { connection_id } if connection_id == id),
    ) {
        return Err(format!(
            "agent config `{}` uses this connection; change or delete it first",
            config.label
        ));
    }
    let before = doc.connections.len();
    doc.connections.retain(|c| c.id != id);
    if doc.connections.len() == before {
        return Err(format!("connection `{id}` not found"));
    }
    for pool in &mut doc.pools {
        pool.members.retain(|m| m.connection_id != id);
    }
    Ok(())
}

pub fn put_pool(doc: &mut ConnectionsDocument, pool: Pool) {
    upsert(&mut doc.pools, pool, |p| &p.id);
}

pub fn delete_pool(doc: &mut ConnectionsDocument, id: &str) -> Result<(), String> {
    if let Some(config) = doc.configs.iter().find(|cfg| match &cfg.target {
        ConfigTarget::Pool { pool_id } => pool_id == id,
        ConfigTarget::Route { pool_id } => pool_id.as_deref() == Some(id),
        ConfigTarget::Connection { .. } => false,
    }) {
        return Err(format!(
            "agent config `{}` uses this pool; change or delete it first",
            config.label
        ));
    }
    let before = doc.pools.len();
    doc.pools.retain(|p| p.id != id);
    if doc.pools.len() == before {
        return Err(format!("pool `{id}` not found"));
    }
    Ok(())
}

/// Q140: same name is never overwritten by a *new* config; replacing an existing
/// id is an edit and is allowed.
pub fn put_config(doc: &mut ConnectionsDocument, config: AgentConfig) -> Result<(), String> {
    let clash = doc.configs.iter().any(|c| {
        c.id != config.id && c.harness == config.harness && c.label.trim() == config.label.trim()
    });
    if clash {
        return Err(format!(
            "an agent config named `{}` already exists for this harness",
            config.label
        ));
    }
    upsert(&mut doc.configs, config, |c| &c.id);
    Ok(())
}

pub fn delete_config(doc: &mut ConnectionsDocument, id: &str) -> Result<(), String> {
    let before = doc.configs.len();
    doc.configs.retain(|c| c.id != id);
    if doc.configs.len() == before {
        return Err(format!("agent config `{id}` not found"));
    }
    Ok(())
}

/// Merge a fetched model list into a connection's catalog (Q134):
/// - existing entries keep every user edit (mapped name, limits, tiers, enabled);
/// - new ids are added as `Fetched` and enabled;
/// - ids that disappeared upstream are kept but disabled, not deleted, so a
///   flaky listing cannot erase hand-tuned entries. Manual entries are untouched.
///
/// Returns `(added, disabled)` counts for the UI.
pub fn merge_fetched_models(connection: &mut Connection, fetched: &[String]) -> (usize, usize) {
    let mut added = 0;
    let mut disabled = 0;
    for id in fetched {
        let id = id.trim();
        if id.is_empty() || connection.models.iter().any(|m| m.upstream_id == id) {
            continue;
        }
        connection.models.push(ModelEntry {
            upstream_id: id.to_string(),
            mapped_name: String::new(),
            display_name: String::new(),
            source: ModelSource::Fetched,
            context_window: None,
            max_output: None,
            inputs: Vec::new(),
            reasoning_tiers: Vec::new(),
            enabled: true,
            protocols: vec![],
        });
        added += 1;
    }
    for entry in &mut connection.models {
        if entry.source == ModelSource::Fetched
            && entry.enabled
            && !fetched.iter().any(|id| id.trim() == entry.upstream_id)
        {
            entry.enabled = false;
            disabled += 1;
        }
    }
    if added > 0 || disabled > 0 {
        connection.catalog_revision += 1;
    }
    (added, disabled)
}
