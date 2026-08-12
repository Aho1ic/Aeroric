use std::time::Duration;

pub(crate) const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_ACTIVE_CONNECTIONS: usize = 512;
pub(crate) const HOST_CONTROL_QUEUE_CAPACITY: usize = 64;
pub(crate) const WEBSOCKET_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const DIAL_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const CONTROL_PING_INTERVAL: Duration = Duration::from_secs(25);
pub(crate) const CONTROL_IDLE_DISCONNECT: Duration = Duration::from_secs(75);

#[derive(Clone)]
pub(crate) struct RelayConfig {
    pub(crate) token: String,
}

impl RelayConfig {
    pub(crate) fn from_env() -> Result<(u16, Self), String> {
        let port = std::env::var("RELAY_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(6791);
        let token = require_relay_token(std::env::var("RELAY_TOKEN").ok())?;
        Ok((port, Self { token }))
    }
}

pub(crate) fn require_relay_token(token: Option<String>) -> Result<String, String> {
    token
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            "RELAY_TOKEN is required and must be non-empty; refusing to start an open relay"
                .to_string()
        })
}
