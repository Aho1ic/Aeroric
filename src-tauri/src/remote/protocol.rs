//! 手机远程连接的控制面协议。
//!
//! E2EE hello/auth、terminal binary framing 与持久化配对仍固定使用 v2。
//! 认证成功后，双方可在加密通道内协商 RPC v3；字段缺失或旧客户端固定
//! 回退 v2，确保已配对设备和旧版本可以长期互通。

use serde::Deserialize;
use serde_json::{json, Value};

/// E2EE/hello 协议版本。它与加密通道内协商的 RPC 版本相互独立。
pub const PROTOCOL_VERSION: u32 = 2;
pub const RPC_V2: u32 = 2;
pub const RPC_V3: u32 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RpcVersion {
    V2,
    V3,
}

impl RpcVersion {
    pub fn as_u32(self) -> u32 {
        match self {
            Self::V2 => RPC_V2,
            Self::V3 => RPC_V3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RpcId {
    Number(u64),
    Text(String),
}

impl RpcId {
    fn value(&self) -> Value {
        match self {
            Self::Number(id) => json!(id),
            Self::Text(id) => json!(id),
        }
    }
}

/// 统一的领域请求。wire 差异在解析边界消失，业务分发无需感知版本。
#[derive(Debug)]
pub struct RpcRequest {
    pub id: RpcId,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcV2Request {
    v: u32,
    id: u64,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcV3Request {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    id: String,
    method: String,
    #[serde(default)]
    params: Value,
}

/// 认证消息始终是 v2。新增协商字段只出现在 params 中。
pub fn parse_auth_request(raw: &str) -> Result<RpcRequest, String> {
    let req: RpcV2Request =
        serde_json::from_str(raw).map_err(|error| format!("Malformed request: {error}"))?;
    if req.v != RPC_V2 {
        return Err(format!("Unsupported auth protocol version {}", req.v));
    }
    Ok(RpcRequest {
        id: RpcId::Number(req.id),
        method: req.method,
        params: req.params,
    })
}

/// 解析认证后的 v2/v3 请求。
pub fn parse_request(raw: &str, negotiated: RpcVersion) -> Result<RpcRequest, String> {
    match negotiated {
        RpcVersion::V2 => parse_auth_request(raw),
        RpcVersion::V3 => {
            let req: RpcV3Request =
                serde_json::from_str(raw).map_err(|error| format!("Malformed request: {error}"))?;
            if req.v != RPC_V3 || req.kind != "request" || req.id.is_empty() {
                return Err("Malformed v3 request envelope".to_string());
            }
            Ok(RpcRequest {
                id: RpcId::Text(req.id),
                method: req.method,
                params: req.params,
            })
        }
    }
}

pub fn select_rpc_version(params: &Value) -> RpcVersion {
    let supports_v3 = params
        .get("supportedRpcVersions")
        .and_then(Value::as_array)
        .is_some_and(|versions| versions.iter().any(|version| version.as_u64() == Some(3)));
    if supports_v3 {
        RpcVersion::V3
    } else {
        RpcVersion::V2
    }
}

pub struct RpcResponse {
    version: RpcVersion,
    id: RpcId,
    result: Result<Value, RpcError>,
}

struct RpcError {
    code: &'static str,
    message: String,
    retryable: bool,
}

impl RpcResponse {
    pub fn success(version: RpcVersion, id: RpcId, result: Value) -> Self {
        Self {
            version,
            id,
            result: Ok(result),
        }
    }

    pub fn failure(version: RpcVersion, id: RpcId, error: impl Into<String>) -> Self {
        Self {
            version,
            id,
            result: Err(RpcError {
                code: "remote_error",
                message: error.into(),
                retryable: false,
            }),
        }
    }

    pub fn malformed(version: RpcVersion, error: impl Into<String>) -> Self {
        let id = match version {
            RpcVersion::V2 => RpcId::Number(0),
            RpcVersion::V3 => RpcId::Text(String::new()),
        };
        Self::failure(version, id, error)
    }

    pub fn to_json(&self) -> String {
        let id = self.id.value();
        let value = match (&self.result, self.version) {
            (Ok(result), RpcVersion::V2) => {
                json!({ "v": RPC_V2, "id": id, "ok": true, "result": result })
            }
            (Err(error), RpcVersion::V2) => {
                json!({ "v": RPC_V2, "id": id, "ok": false, "error": error.message })
            }
            (Ok(result), RpcVersion::V3) => json!({
                "v": RPC_V3,
                "type": "response",
                "id": id,
                "ok": true,
                "result": result,
            }),
            (Err(error), RpcVersion::V3) => json!({
                "v": RPC_V3,
                "type": "response",
                "id": id,
                "ok": false,
                "error": {
                    "code": error.code,
                    "message": error.message,
                    "retryable": error.retryable,
                },
            }),
        };
        serde_json::to_string(&value).unwrap_or_else(|_| {
            json!({
                "v": self.version.as_u32(),
                "id": id,
                "ok": false,
                "error": "serialize failed",
            })
            .to_string()
        })
    }
}

pub fn push_json(version: RpcVersion, event: &str, seq: Option<u64>, data: &Value) -> String {
    match version {
        RpcVersion::V2 => json!({
            "v": RPC_V2,
            "push": event,
            "seq": seq,
            "data": data,
        }),
        RpcVersion::V3 => json!({
            "v": RPC_V3,
            "type": "push",
            "event": event,
            "seq": seq,
            "data": data,
        }),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_wire_shape_matches_shared_golden_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../packages/remote-contracts/fixtures/rpc-golden.json"
        ))
        .expect("shared golden fixture");
        let request = parse_request(&fixture["request"].to_string(), RpcVersion::V3)
            .expect("parse v3 request");
        assert_eq!(request.id, RpcId::Text("rpc-7".to_string()));

        let response = RpcResponse::success(RpcVersion::V3, request.id, json!([]));
        let response_value: Value = serde_json::from_str(&response.to_json()).expect("response");
        assert_eq!(response_value, fixture["success"]);

        let push = push_json(
            RpcVersion::V3,
            "task-status",
            Some(42),
            &json!({ "task_id": "task-1", "status": "running" }),
        );
        assert_eq!(
            serde_json::from_str::<Value>(&push).expect("push"),
            fixture["push"]
        );
    }

    #[test]
    fn negotiation_defaults_to_v2_for_old_clients() {
        assert_eq!(select_rpc_version(&json!({})), RpcVersion::V2);
        assert_eq!(
            select_rpc_version(&json!({ "supportedRpcVersions": [3, 2] })),
            RpcVersion::V3
        );
    }
}
