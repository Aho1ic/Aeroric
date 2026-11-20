//! C8 (2026-11-20): the context tools with a side effect — `delegate`,
//! `board_write`, `board_decide`, `memory_propose` — run once per
//! idempotency key. `aeroric-cli mcp-context` puts the agent's key (or one
//! it made) in the request envelope; a repeated key from the same task gets
//! the first call's result, marked `replayed`, without running again. So an
//! agent whose call timed out, or whose server lost the host mid-call, can
//! call again without delegating twice.
//!
//! Kept on disk (`<data>/context-once.json`) for a day, so it holds across
//! a host restart; only successes are kept — a refusal may be fixed and
//! sent again.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use aeroric_protocol::envelope::{Frame, Success};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Live;
use crate::dispatcher::{Handler, ServiceSet};

/// The methods behind the keyed tools.
pub(super) const METHODS: [&str; 4] = [
    super::delegation::CONTEXT_DELEGATE_METHOD,
    super::board::CONTEXT_BOARD_WRITE_METHOD,
    super::board::CONTEXT_BOARD_DECIDE_METHOD,
    super::memory::CONTEXT_MEMORY_PROPOSE_METHOD,
];
/// How long a key is remembered.
const KEPT_MS: u64 = 24 * 60 * 60 * 1000;
/// At most this many keys; the oldest go first.
const MAX_KEYS: usize = 2000;

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    at_ms: u64,
    result: Value,
}

pub(super) struct OnceStore {
    path: PathBuf,
    entries: Mutex<HashMap<String, Entry>>,
}

impl OnceStore {
    pub(super) fn open(path: PathBuf) -> Self {
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
        Self {
            path,
            entries: Mutex::new(entries),
        }
    }

    pub(super) fn get(&self, slot: &str, now_ms: u64) -> Option<Value> {
        self.entries
            .lock()
            .get(slot)
            .filter(|e| now_ms.saturating_sub(e.at_ms) < KEPT_MS)
            .map(|e| e.result.clone())
    }

    pub(super) fn put(&self, slot: String, result: Value, now_ms: u64) {
        let mut entries = self.entries.lock();
        entries.retain(|_, e| now_ms.saturating_sub(e.at_ms) < KEPT_MS);
        if entries.len() >= MAX_KEYS {
            if let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, e)| e.at_ms)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&oldest);
            }
        }
        entries.insert(
            slot,
            Entry {
                at_ms: now_ms,
                result,
            },
        );
        let raw = serde_json::to_vec(&*entries).unwrap_or_default();
        drop(entries);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = aeroric_core::storage::atomic::write_atomic(
            &self.path,
            &raw,
            aeroric_core::storage::atomic::FileMode::Private,
        ) {
            eprintln!("[aeroric-host] idempotency keys not kept: {error}");
        }
    }
}

pub(super) fn install(set: &mut ServiceSet, live: &Arc<Live>) {
    let store = Arc::new(OnceStore::open(
        live.context.paths.data_dir().join("context-once.json"),
    ));
    for method in METHODS {
        if let Some(inner) = set.take(method) {
            set.register(method, once(live, &store, method, inner));
        }
    }
}

fn once(live: &Arc<Live>, store: &Arc<OnceStore>, method: &'static str, inner: Handler) -> Handler {
    let live = Arc::clone(live);
    let store = Arc::clone(store);
    Arc::new(move |request, call| {
        // Scoped to the task the token names: two sessions never share a key.
        let slot = request.idempotency_key.as_ref().and_then(|key| {
            let token = request.params.get("token").and_then(Value::as_str)?;
            let (task, _) = live.executor.context_tokens().resolve(token)?;
            Some(format!("{method}\n{task}\n{key}"))
        });
        let inner = Arc::clone(&inner);
        let store = Arc::clone(&store);
        Box::pin(async move {
            let now = super::now_ms();
            if let Some(slot) = &slot {
                if let Some(mut result) = store.get(slot, now) {
                    if let Some(object) = result.as_object_mut() {
                        object.insert("replayed".into(), Value::Bool(true));
                    }
                    return Frame::Success(Success {
                        v: aeroric_protocol::PROTOCOL_VERSION,
                        id: request.id.clone(),
                        ok: true,
                        result,
                        extra: Default::default(),
                    });
                }
            }
            let frame = inner(request, call).await;
            if let (Some(slot), Frame::Success(success)) = (slot, &frame) {
                store.put(slot, success.result.clone(), now);
            }
            frame
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key is answered from disk after a restart, forgotten after a day,
    /// and the oldest goes when there are too many.
    #[test]
    fn keys_survive_a_restart_and_expire() {
        let path = std::env::temp_dir()
            .join(format!("aeroric-once-{}", uuid::Uuid::new_v4()))
            .join("context-once.json");
        let store = OnceStore::open(path.clone());
        store.put("m\nt\nk".into(), serde_json::json!({"id": "b1"}), 10);
        assert_eq!(
            store.get("m\nt\nk", 11),
            Some(serde_json::json!({"id": "b1"}))
        );
        let again = OnceStore::open(path.clone());
        assert_eq!(
            again.get("m\nt\nk", 12),
            Some(serde_json::json!({"id": "b1"}))
        );
        assert_eq!(again.get("m\nt\nk", 10 + KEPT_MS), None, "a day later");
        assert_eq!(again.get("m\nother\nk", 12), None, "another task");
        for at in 0..MAX_KEYS as u64 {
            again.put(format!("m\nt\n{at}"), serde_json::json!(at), 100 + at);
        }
        assert!(again.entries.lock().len() <= MAX_KEYS);
        assert_eq!(again.get("m\nt\nk", 200), None, "the oldest went");
    }
}
