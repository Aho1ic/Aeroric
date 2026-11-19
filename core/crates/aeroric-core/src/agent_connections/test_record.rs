//! C5 step 2 (2026-11-19): the last test of each connection and model, with
//! the exit it took — the endpoint, the protocol, the proxy and a fingerprint
//! of the key. Read back against the connection as it is now: a test taken
//! through another exit reads "配置已变，需要重测" and says what changed.
//!
//! The key itself is never written: only the first 12 hex digits of its
//! SHA-256, enough to tell that it changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::model::{Connection, WireProtocol};
use crate::settings::model_detect::connection_proxy;
use crate::settings::normalize::normalize_proxy_url;
use crate::settings::schema::ProxySettings;

const FILE: &str = "connection-tests.json";

/// How a test reached its connection.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Egress {
    pub base_url: String,
    /// `responses` | `messages` | `chat_completions`.
    pub protocol: String,
    /// The proxy URL without any user or password; empty: direct.
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub no_proxy: String,
    /// The proxy's user name (its password is never kept).
    #[serde(default)]
    pub proxy_user: String,
    /// First 12 hex digits of the key's SHA-256.
    #[serde(default)]
    pub credential: String,
}

impl Egress {
    /// The exit `connection` takes over `protocol` with key `secret` under
    /// the global proxy settings, decided as the requests themselves decide
    /// it (`connection_proxy`).
    pub fn of(
        connection: &Connection,
        protocol: &str,
        secret: &str,
        global: &ProxySettings,
    ) -> Result<Self, String> {
        let proxy = connection_proxy(connection.use_global_proxy, global)?;
        Ok(Self {
            base_url: connection.base_url.trim().trim_end_matches('/').to_string(),
            protocol: protocol.to_string(),
            proxy: without_userinfo(&normalize_proxy_url(&proxy.url)),
            no_proxy: proxy.no_proxy.trim().to_string(),
            proxy_user: proxy.username.trim().to_string(),
            credential: fingerprint(secret),
        })
    }

    /// What differs from `now`, in words; `None`: the same exit.
    pub fn changed(&self, now: &Egress) -> Option<&'static str> {
        if self.base_url != now.base_url {
            Some("地址改过")
        } else if self.protocol != now.protocol {
            Some("协议改过")
        } else if (&self.proxy, &self.no_proxy, &self.proxy_user)
            != (&now.proxy, &now.no_proxy, &now.proxy_user)
        {
            Some("代理设置改过")
        } else if self.credential != now.credential {
            Some("凭据换过")
        } else {
            None
        }
    }
}

/// One test, as it ended.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestRecord {
    pub connection_id: String,
    pub model: String,
    pub at_ms: u64,
    /// The upstream's HTTP status; 0 when no answer came (it could not be
    /// reached, or nothing was sent).
    pub status: u16,
    pub elapsed_ms: u64,
    /// The start of the reply, or why it failed.
    #[serde(default)]
    pub excerpt: String,
    pub egress: Egress,
}

impl TestRecord {
    pub fn passed(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// A test as read now: `stale` says why it no longer describes the exit.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TestView {
    #[serde(flatten)]
    pub record: TestRecord,
    pub passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<String>,
}

pub const NEEDS_RETEST: &str = "配置已变，需要重测";

pub fn fingerprint(secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return String::new();
    }
    Sha256::digest(secret.as_bytes())
        .iter()
        .take(6)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn without_userinfo(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() || parsed.password().is_some() => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string().trim_end_matches('/').to_string()
        }
        _ => url.to_string(),
    }
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

fn key(connection_id: &str, model: &str) -> String {
    format!("{connection_id}\u{1f}{model}")
}

fn load_all(data_dir: &Path) -> BTreeMap<String, TestRecord> {
    std::fs::read(path(data_dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<TestRecord>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|r| (key(&r.connection_id, &r.model), r))
        .collect()
}

fn store_all(data_dir: &Path, all: &BTreeMap<String, TestRecord>) -> Result<(), String> {
    let list: Vec<&TestRecord> = all.values().collect();
    let bytes = serde_json::to_vec_pretty(&list).map_err(|e| e.to_string())?;
    crate::storage::atomic::write_atomic(
        &path(data_dir),
        &bytes,
        crate::storage::atomic::FileMode::Private,
    )
    .map_err(|e| e.to_string())
}

/// Keep `record` as the last test of its connection and model.
pub fn save(data_dir: &Path, record: TestRecord) -> Result<(), String> {
    let mut all = load_all(data_dir);
    all.insert(key(&record.connection_id, &record.model), record);
    store_all(data_dir, &all)
}

/// A deleted connection's tests go with it.
pub fn forget(data_dir: &Path, connection_id: &str) -> Result<(), String> {
    let mut all = load_all(data_dir);
    let before = all.len();
    all.retain(|_, r| r.connection_id != connection_id);
    if all.len() == before {
        return Ok(());
    }
    store_all(data_dir, &all)
}

/// Every kept test of the connections that still exist, each judged
/// against the exit its connection takes now. `secret_of` gives a
/// connection's key (`None`: none stored).
pub fn views(
    data_dir: &Path,
    connections: &[Connection],
    global: &ProxySettings,
    secret_of: impl Fn(&str) -> Option<String>,
) -> Vec<TestView> {
    load_all(data_dir)
        .into_values()
        .filter_map(|record| {
            let connection = connections.iter().find(|c| c.id == record.connection_id)?;
            let speaks = connection
                .protocols
                .iter()
                .any(|p| p.as_str() == record.egress.protocol);
            let secret = secret_of(&connection.id).unwrap_or_default();
            let changed = if !speaks && !connection.protocols.is_empty() {
                Some("协议改过".to_string())
            } else {
                match Egress::of(connection, &record.egress.protocol, &secret, global) {
                    Ok(now) => record.egress.changed(&now).map(str::to_string),
                    Err(reason) => Some(format!("代理设置改过：{reason}")),
                }
            };
            Some(TestView {
                passed: record.passed(),
                stale: changed.map(|why| format!("{NEEDS_RETEST}（{why}）")),
                record,
            })
        })
        .collect()
}

/// The protocol a test uses: Chat Completions when offered (cheapest to
/// read back), else the connection's first.
pub fn test_protocol(connection: &Connection) -> &'static str {
    if connection
        .protocols
        .contains(&WireProtocol::ChatCompletions)
    {
        "chat_completions"
    } else {
        connection
            .protocols
            .first()
            .map(|p| p.as_str())
            .unwrap_or("chat_completions")
    }
}

#[cfg(test)]
#[path = "test_record_tests.rs"]
mod tests;
