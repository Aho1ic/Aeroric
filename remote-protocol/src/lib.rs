//! Aeroric 远程访问共享协议类型:桌面端(src-tauri)与自托管 relay
//! (remote-relay)共用的撮合控制面消息与 WS 路由约定。
//!
//! relay 对业务流量完全盲转发——手机与桌面之间是 E2EE 密文(见桌面端
//! remote/crypto.rs),relay 既不持有密钥也不解析转发内容;本 crate 只覆盖
//! 「桌面注册 / 客户端接入 / 数据连接撮合」这三步控制面。
//!
//! 连接拓扑(全部为出站/入站 WS):
//!
//! ```text
//! 桌面 ──(出站)──► wss://relay/host          控制连接:注册 hostId,等 ClientConnected
//! 手机 ──(出站)──► wss://relay/connect/<hostId>  接入:relay 生成 connId 通知桌面
//! 桌面 ──(出站)──► wss://relay/data/<connId>  数据连接:relay 与手机连接逐帧对接
//! ```

use serde::{Deserialize, Serialize};

pub const RELAY_PROTOCOL_VERSION: u32 = 1;

/// 桌面 → relay(控制连接首条 text 消息)。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostToRelay {
    Register {
        v: u32,
        host_id: String,
        /// relay 部署方通过必填的 RELAY_TOKEN 环境变量设置的共享口令。
        /// Option 仅用于兼容旧客户端并让 relay 明确拒绝缺失口令。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
}

/// relay → 桌面(控制连接 text 消息)。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayToHost {
    Registered,
    ClientConnected {
        conn_id: String,
        /// 客户端对 relay 的对端地址(桌面用于认证限流与审计)。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer: Option<String>,
    },
    Error {
        message: String,
    },
}

/// WS 路由(路径约定,双方共用)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayRoute {
    HostControl,
    ClientConnect { host_id: String },
    HostData { conn_id: String },
}

/// 路径段合法性:base64url / uuid 字符集,防路径注入与超长滥用。
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn parse_route(path: &str) -> Option<RelayRoute> {
    if path == "/host" {
        return Some(RelayRoute::HostControl);
    }
    if let Some(host_id) = path.strip_prefix("/connect/") {
        if valid_id(host_id) {
            return Some(RelayRoute::ClientConnect {
                host_id: host_id.to_string(),
            });
        }
        return None;
    }
    if let Some(conn_id) = path.strip_prefix("/data/") {
        if valid_id(conn_id) {
            return Some(RelayRoute::HostData {
                conn_id: conn_id.to_string(),
            });
        }
        return None;
    }
    None
}

pub fn host_control_url(base: &str) -> String {
    format!("{}/host", base.trim_end_matches('/'))
}

pub fn client_connect_url(base: &str, host_id: &str) -> String {
    format!("{}/connect/{host_id}", base.trim_end_matches('/'))
}

pub fn host_data_url(base: &str, conn_id: &str) -> String {
    format!("{}/data/{conn_id}", base.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_roundtrip_with_urls() {
        assert_eq!(parse_route("/host"), Some(RelayRoute::HostControl));
        assert_eq!(
            parse_route("/connect/abc-DEF_123"),
            Some(RelayRoute::ClientConnect {
                host_id: "abc-DEF_123".to_string()
            })
        );
        assert_eq!(
            parse_route("/data/550e8400-e29b-41d4-a716-446655440000"),
            Some(RelayRoute::HostData {
                conn_id: "550e8400-e29b-41d4-a716-446655440000".to_string()
            })
        );
        assert_eq!(parse_route("/connect/"), None);
        assert_eq!(parse_route("/connect/a/b"), None);
        assert_eq!(parse_route("/other"), None);

        assert_eq!(
            host_control_url("wss://r.example.com/"),
            "wss://r.example.com/host"
        );
        assert_eq!(
            client_connect_url("ws://1.2.3.4:6791", "h1"),
            "ws://1.2.3.4:6791/connect/h1"
        );
        assert_eq!(
            host_data_url("ws://1.2.3.4:6791", "c1"),
            "ws://1.2.3.4:6791/data/c1"
        );
    }

    #[test]
    fn control_messages_serialize_stably() {
        let register = HostToRelay::Register {
            v: RELAY_PROTOCOL_VERSION,
            host_id: "h1".to_string(),
            token: None,
        };
        let json = serde_json::to_string(&register).unwrap();
        assert_eq!(json, r#"{"type":"register","v":1,"host_id":"h1"}"#);
        assert_eq!(
            serde_json::from_str::<HostToRelay>(&json).unwrap(),
            register
        );

        let connected: RelayToHost = serde_json::from_str(
            r#"{"type":"client_connected","conn_id":"c1","peer":"1.2.3.4:5"}"#,
        )
        .unwrap();
        assert_eq!(
            connected,
            RelayToHost::ClientConnected {
                conn_id: "c1".to_string(),
                peer: Some("1.2.3.4:5".to_string())
            }
        );
    }
}
