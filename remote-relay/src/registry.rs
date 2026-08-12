use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::rate_limit::ClientRateLimit;
use crate::Ws;

pub(crate) const MAX_PENDING_CONNECTIONS: usize = 256;
pub(crate) const MAX_PENDING_PER_HOST: usize = 32;

#[derive(Default)]
pub(crate) struct Registry {
    pub(crate) hosts: Mutex<HashMap<String, mpsc::Sender<Message>>>,
    pub(crate) pending: Mutex<HashMap<String, PendingConnection>>,
    pub(crate) client_rate_limits: Mutex<HashMap<IpAddr, ClientRateLimit>>,
}

pub(crate) struct PendingConnection {
    pub(crate) host_id: String,
    pub(crate) sender: oneshot::Sender<Ws>,
}

pub(crate) fn try_register_host(
    registry: &Registry,
    host_id: &str,
    sender: mpsc::Sender<Message>,
) -> bool {
    let mut hosts = registry.hosts.lock().unwrap();
    if hosts.contains_key(host_id) {
        return false;
    }
    hosts.insert(host_id.to_string(), sender);
    true
}

pub(crate) fn cleanup_host(registry: &Registry, host_id: &str, sender: &mpsc::Sender<Message>) {
    let mut hosts = registry.hosts.lock().unwrap();
    if hosts
        .get(host_id)
        .is_some_and(|current| current.same_channel(sender))
    {
        hosts.remove(host_id);
    }
}

pub(crate) fn try_insert_pending(
    registry: &Registry,
    host_id: &str,
    conn_id: &str,
    sender: oneshot::Sender<Ws>,
) -> bool {
    let mut pending = registry.pending.lock().unwrap();
    if pending.len() >= MAX_PENDING_CONNECTIONS
        || pending
            .values()
            .filter(|entry| entry.host_id == host_id)
            .count()
            >= MAX_PENDING_PER_HOST
    {
        return false;
    }
    pending.insert(
        conn_id.to_string(),
        PendingConnection {
            host_id: host_id.to_string(),
            sender,
        },
    );
    true
}
