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

    /// 校验注册方带来的 token,且不按字节提前返回。
    ///
    /// `==` 会在第一个不同的字节上短路,泄漏"猜对了几位前缀"这一信号。这里比的是
    /// 共享口令本身(不是摘要),猜对前缀就是直接进展,所以这条侧信道比
    /// `remote/auth.rs` 的摘要比较更值得堵 —— 而 relay 是本项目唯一按设计暴露在
    /// 公网的组件。长度不同仍会被观察到,和 `constant_time_hash_eq` 的取舍一致:
    /// 泄漏长度远弱于逐字节泄漏内容,换取不引入额外依赖。
    pub(crate) fn token_matches(&self, provided: Option<&str>) -> bool {
        let Some(provided) = provided else {
            return false;
        };
        let expected = self.token.as_bytes();
        let provided = provided.as_bytes();
        if expected.is_empty() || expected.len() != provided.len() {
            return false;
        }
        expected
            .iter()
            .zip(provided.iter())
            .fold(0_u8, |difference, (l, r)| difference | (l ^ r))
            == 0
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
