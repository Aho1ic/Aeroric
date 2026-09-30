//! 连接状态机与事件订阅（P3-06 Action 3 的订阅半边）。
//!
//! 状态只有两格：未认证 → 已认证（带 clientId）。事件订阅的 watermark
//! 直接复用 core `events::snapshot::SnapshotDecision`——**补发/直播的去重
//! 语义全仓只有那一份实现**，这里不写第二份「比大小」。

use aeroric_core::events::snapshot::SnapshotDecision;

/// 一条连接的服务端视角状态。
#[derive(Debug)]
pub struct Connection {
    state: ConnectionState,
    pub snapshot: SnapshotDecision,
}

#[derive(Debug)]
enum ConnectionState {
    Unauthed,
    Authed { client_id: String },
}

impl Connection {
    pub const HANDSHAKE_METHOD: &'static str = "handshake";

    pub fn new() -> Self {
        Self {
            state: ConnectionState::Unauthed,
            snapshot: SnapshotDecision::at(0),
        }
    }

    pub fn is_authenticated(&self) -> bool {
        matches!(self.state, ConnectionState::Authed { .. })
    }

    pub fn client_id(&self) -> Option<&str> {
        match &self.state {
            ConnectionState::Unauthed => None,
            ConnectionState::Authed { client_id } => Some(client_id),
        }
    }

    /// 握手成功后翻状态。**epoch 语义**（Action 4「旧 epoch 拒绝」）：
    /// 已认证连接再握手一次 = 换 host 的信号（重连走新连接，不在旧连接上
    /// 重握手），返回 false 让 dispatcher 拒绝。
    pub fn mark_authenticated(&mut self, client_id: String) -> bool {
        if self.is_authenticated() {
            return false;
        }
        self.state = ConnectionState::Authed { client_id };
        true
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_flips_once_and_names_the_client() {
        let mut connection = Connection::new();
        assert!(!connection.is_authenticated());
        assert_eq!(connection.client_id(), None);
        assert!(connection.mark_authenticated("c1".to_string()));
        assert!(connection.is_authenticated());
        assert_eq!(connection.client_id(), Some("c1"));
        assert!(
            !connection.mark_authenticated("c2".to_string()),
            "第二次握手必须失败"
        );
        assert_eq!(connection.client_id(), Some("c1"), "身份不被覆盖");
    }

    #[test]
    fn watermark_starts_at_the_no_watermark_sentinel() {
        let connection = Connection::new();
        assert_eq!(connection.snapshot.watermark, 0);
        assert!(!connection.snapshot.needs_full_refresh);
    }
}
