//! 手机远程连接的线协议(v2:E2EE 强制,见 crypto.rs)。
//!
//! 明文阶段仅有握手两条 text 帧(hello / hello_ack);此后所有消息都封在
//! 加密二进制帧里。控制面(kind=1)内层仍是 JSON,三种消息:
//! - 请求:  `{"v":2,"id":<u64>,"method":"...","params":{...}}`
//! - 响应:  `{"v":2,"id":<u64>,"ok":true,"result":...}` / `{"v":2,"id":<u64>,"ok":false,"error":"..."}`
//! - 推送:  `{"v":2,"push":"<event>","data":...}`(服务端→客户端单向)
//!
//! v1(明文 JSON)已废弃:收到旧版本 hello/请求一律拒连,不做隐式降级。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 2;

/// 客户端→服务端请求。
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub v: u32,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// 服务端→客户端响应。
#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub v: u32,
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RpcResponse {
    pub fn success(id: u64, result: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: u64, error: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                "{{\"v\":{PROTOCOL_VERSION},\"id\":{},\"ok\":false,\"error\":\"serialize failed\"}}",
                self.id
            )
        })
    }
}

/// 服务端→客户端推送(桌面事件桥转发 task-status 等)。
/// `seq` 仅在事件进了 event_log(可补发)时携带,客户端以此维护 watermark。
#[derive(Debug, Serialize)]
pub struct RpcPush<'a> {
    pub v: u32,
    pub push: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    pub data: Value,
}

impl<'a> RpcPush<'a> {
    pub fn new(event: &'a str, data: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            push: event,
            seq: None,
            data,
        }
    }

    pub fn with_seq(event: &'a str, seq: u64, data: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            push: event,
            seq: Some(seq),
            data,
        }
    }

    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }
}

/// 解析请求;协议版本不匹配返回 Err(错误文案随响应回给客户端)。
pub fn parse_request(raw: &str) -> Result<RpcRequest, String> {
    let req: RpcRequest =
        serde_json::from_str(raw).map_err(|e| format!("Malformed request: {e}"))?;
    if req.v != PROTOCOL_VERSION {
        return Err(format!(
            "Unsupported protocol version {} (server speaks {PROTOCOL_VERSION})",
            req.v
        ));
    }
    Ok(req)
}
