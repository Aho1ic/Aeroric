//! D4 first step (2026-11-21): commands from a phone carry an identity.
//!
//! A command — a method that changes something on the Mac — may carry
//! `commandId` (made on the device, the same when it is sent again) and
//! `hostEpoch` (the host start it was based on, from `hello`):
//!
//! - the host keeps each device's command results for [`KEEP_MS`]: the same
//!   `commandId` again gets the first result, and is not run twice — even
//!   while the first is still running (the second waits for it);
//! - a `commandId` used again with other params is refused;
//! - a `hostEpoch` that is not this host start's is refused as stale: the
//!   phone resyncs and decides again;
//! - a revoked device's results are dropped, and its commands refused.
//!
//! Reads (lists, files, diffs) are not commands and pass as before; so do
//! commands without a `commandId` (older phones).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

/// How long a result is kept for a resend.
pub const KEEP_MS: u64 = 10 * 60 * 1000;
/// Results kept per device (the oldest go first).
pub const PER_DEVICE: usize = 500;
const MAX_ID_CHARS: usize = 128;

/// The methods that change something on the Mac.
pub const COMMANDS: &[&str] = &[
    "projects.setPinned",
    "project.writeFile",
    "agentConfig.save",
    "agentConfig.create",
    "task.input",
    "task.respond",
    "task.cancel",
    "task.complete",
    "task.create",
    "task.resume",
    "push.register",
    "push.test",
];

/// The prefix of a stale-command refusal (the phone matches it).
pub const STALE: &str = "STALE_HOST_EPOCH";

type Outcome = Result<Value, String>;

struct Entry {
    fingerprint: String,
    at_ms: u64,
    outcome: Arc<OnceCell<Outcome>>,
}

/// Each device's recent commands, by `commandId`.
#[derive(Default)]
pub struct CommandLedger {
    devices: Mutex<HashMap<String, HashMap<String, Entry>>>,
}

/// What to do with one request.
pub enum Admit {
    /// Not a command, or one with no `commandId`: run it as before.
    Run,
    /// Run it once through this cell (others with the same id wait on it).
    Once(Arc<OnceCell<Outcome>>),
    Refuse(String),
}

fn fingerprint(method: &str, params: &Value) -> String {
    let mut params = params.clone();
    if let Some(map) = params.as_object_mut() {
        map.remove("commandId");
        map.remove("hostEpoch");
    }
    let digest = Sha256::digest(format!("{method}\n{params}").as_bytes());
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

impl CommandLedger {
    /// Decide for one request of `device`.
    pub fn admit(
        &self,
        device: &str,
        method: &str,
        params: &Value,
        host_epoch: &str,
        now_ms: u64,
    ) -> Admit {
        if !COMMANDS.contains(&method) {
            return Admit::Run;
        }
        if let Some(based) = params.get("hostEpoch").and_then(Value::as_str) {
            if based != host_epoch {
                return Admit::Refuse(format!(
                    "{STALE}: 这条命令基于宿主的上一次启动，宿主已经重启；重新同步后再决定"
                ));
            }
        }
        let Some(id) = params.get("commandId").and_then(Value::as_str) else {
            return Admit::Run;
        };
        if id.is_empty() || id.chars().count() > MAX_ID_CHARS {
            return Admit::Refuse("commandId 不合法".into());
        }
        let print = fingerprint(method, params);
        let mut devices = self.devices.lock();
        let mine = devices.entry(device.to_string()).or_default();
        mine.retain(|_, e| now_ms.saturating_sub(e.at_ms) < KEEP_MS);
        if let Some(entry) = mine.get(id) {
            if entry.fingerprint != print {
                return Admit::Refuse("这个 commandId 已经用在了另一条命令上".into());
            }
            return Admit::Once(Arc::clone(&entry.outcome));
        }
        if mine.len() >= PER_DEVICE {
            if let Some(oldest) = mine
                .iter()
                .min_by_key(|(_, e)| e.at_ms)
                .map(|(k, _)| k.clone())
            {
                mine.remove(&oldest);
            }
        }
        let outcome = Arc::new(OnceCell::new());
        mine.insert(
            id.to_string(),
            Entry {
                fingerprint: print,
                at_ms: now_ms,
                outcome: Arc::clone(&outcome),
            },
        );
        Admit::Once(outcome)
    }

    /// A revoked device: its kept results go.
    pub fn forget_device(&self, device: &str) {
        self.devices.lock().remove(device);
    }

    /// Results kept for `device` (tests, diagnostics).
    pub fn kept(&self, device: &str) -> usize {
        self.devices.lock().get(device).map_or(0, HashMap::len)
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
