use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio_tungstenite::tungstenite::handshake::server::Request;

pub(crate) const CLIENT_CONNECT_RATE_LIMIT: u32 = 12;
pub(crate) const CLIENT_CONNECT_RATE_WINDOW: Duration = Duration::from_secs(10);
const CLIENT_RATE_LIMIT_RETENTION: Duration = Duration::from_secs(10 * 60);
const MAX_CLIENT_RATE_LIMIT_ENTRIES: usize = 4096;

pub(crate) struct ClientRateLimit {
    window_started: Instant,
    attempts: u32,
    last_seen: Instant,
}

pub(crate) fn source_ip(peer: IpAddr, request: &Request) -> IpAddr {
    if !peer.is_loopback() {
        return peer;
    }
    ["x-forwarded-for", "x-real-ip"]
        .iter()
        .filter_map(|name| request.headers().get(*name))
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.rsplit(',').next())
        .find_map(|value| value.trim().parse().ok())
        .unwrap_or(peer)
}

pub(crate) fn try_acquire(
    rate_limits: &Mutex<HashMap<IpAddr, ClientRateLimit>>,
    peer: IpAddr,
    now: Instant,
) -> bool {
    let mut limits = rate_limits.lock().unwrap();
    if limits.len() >= MAX_CLIENT_RATE_LIMIT_ENTRIES {
        limits.retain(|_, entry| now.duration_since(entry.last_seen) < CLIENT_RATE_LIMIT_RETENTION);
        if limits.len() >= MAX_CLIENT_RATE_LIMIT_ENTRIES && !limits.contains_key(&peer) {
            if let Some(oldest) = limits
                .iter()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(address, _)| *address)
            {
                limits.remove(&oldest);
            }
        }
    }

    let entry = limits.entry(peer).or_insert(ClientRateLimit {
        window_started: now,
        attempts: 0,
        last_seen: now,
    });
    entry.last_seen = now;
    if now.duration_since(entry.window_started) >= CLIENT_CONNECT_RATE_WINDOW {
        entry.window_started = now;
        entry.attempts = 0;
    }
    if entry.attempts >= CLIENT_CONNECT_RATE_LIMIT {
        return false;
    }
    entry.attempts += 1;
    true
}
