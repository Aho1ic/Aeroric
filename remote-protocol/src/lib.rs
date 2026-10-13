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
//!
//! AP6 起本 crate 还持有**推送平面**(Mac 宿主 → 自托管 APNs 发送端)的 wire
//! 类型:`PushEnvelope`/`PushRequest`/`PushAck`/`RelayDiagnostics`。它们与
//! relay 控制面并列,但走 HTTPS 而不是 WS,并且**绝不**承载 APNs 私钥。

use serde::{Deserialize, Serialize};

pub const RELAY_PROTOCOL_VERSION: u32 = 1;

/// 推送平面协议版本。手机/宿主与发送端各自独立部署,所以版本是显式字段。
pub const PUSH_PROTOCOL_VERSION: u32 = 1;

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

// ── AP6 推送平面(Mac 宿主 → 自托管 APNs 发送端)──────────────────────────────
//
// 边界(Q7/Q10 + MIGRATION_PLAN §D AP6):
//   * APNs 私钥(.p8)只存在于发送端;**任何**出现在这里的类型都不含私钥字段,
//     这条由 `remote-push` 的测试断言(客户端源码里没有 p8/AuthKey 读取面)。
//   * 载荷最小化:信封只含 `kind`/`eventSeq`/`collapseKey`;正文由发送端按
//     `kind` 构造 `aps.alert.loc-key`,文案留在手机 bundle,不经过网络。
//   * 宿主按 **device id** 推送,APNs device token 只在 `push.register` 时
//     单向上报一次并由发送端保存(见 `PushBindRequest`),之后不再出现在线路上。

/// 允许触发外部通知的事件种类。**只有三种**:任务完成/失败/等待审批。
/// 其它状态(`running`/`pending` 等)不产生通知——通知是提醒,不是状态同步。
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushKind {
    TaskDone,
    TaskFailed,
    ApprovalPending,
}

impl PushKind {
    /// 发送端据此构造 `aps.alert.loc-key`;`loc-key` 是客户端 bundle 里的键名,
    /// 因此文案不进网络、可本地化,也不泄露业务内容。
    pub fn loc_key(self) -> &'static str {
        match self {
            Self::TaskDone => "push.task_done",
            Self::TaskFailed => "push.task_failed",
            Self::ApprovalPending => "push.approval_pending",
        }
    }

    /// APNs `apns-collapse-id` 的类别段:同一任务+同一类别的重复通知合并。
    pub fn collapse_tag(self) -> &'static str {
        match self {
            Self::TaskDone => "done",
            Self::TaskFailed => "failed",
            Self::ApprovalPending => "approval",
        }
    }

    /// 该 kind 的稳定 wire 名(与 serde 的 `snake_case` 表示一致,签名串与
    /// 载荷正文共用同一组字面量)。
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::TaskDone => "task_done",
            Self::TaskFailed => "task_failed",
            Self::ApprovalPending => "approval_pending",
        }
    }
}

/// 宿主 → 发送端的通知信封。**字段集合就是载荷白名单**:没有任务标题、项目名、
/// 路径、命令或提示词的容身之处,新增字段必须先改这里并同步最小化测试。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushEnvelope {
    pub v: u32,
    pub kind: PushKind,
    /// 宿主 `EventLog` 的 seq。手机拿它与本地 watermark 比较:**推送本身不改状态**,
    /// 它只说明「有值得重核对的变化」,必须回 E2EE 连接拉快照才算数。
    pub event_seq: u64,
    /// APNs 折叠键(≤64 字节)。不含任务标题,只含任务 id 的哈希段与类别。
    pub collapse_key: String,
}

/// 手机经 E2EE `push.register` 上报的绑定请求(宿主转发给发送端)。
/// APNs device token 只在这里出现一次,发送端保存后即不再传输。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushBindRequest {
    pub v: u32,
    /// 配对派生的 per-user 凭据 id(不是账号;见 ADR-040 §2)。
    pub user: String,
    /// 宿主侧设备 id(`remote-devices.json` 的 `id`),推送与撤销都按它寻址。
    pub device_id: String,
    /// "ios" / "ipados"。发送端据此选择 topic 与 APNs 环境。
    pub platform: String,
    /// APNs device token(hex)。空字符串 = 注销该设备的绑定。
    pub token: String,
    pub issued_at_ms: i64,
    pub nonce: String,
    pub signature: String,
}

/// 宿主 → 发送端的推送请求。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub v: u32,
    pub user: String,
    /// 目标设备 id 列表;发送端查自己的绑定表得到 token,**宿主从不携带 token**。
    pub device_ids: Vec<String>,
    pub envelope: PushEnvelope,
    pub issued_at_ms: i64,
    pub nonce: String,
    pub signature: String,
}

/// 发送端应答。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushAck {
    pub v: u32,
    /// 已被 APNs 接受(202)的推送数。
    pub accepted: usize,
    /// APNs 判定失效并已解绑的设备(410 Unregistered / BadDeviceToken)。
    pub invalidated_device_ids: Vec<String>,
    /// 建议宿主在多少毫秒内不要再推(429/503)。`None` = 无退避要求。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

/// 推送平面错误。`code` 是稳定判别式,`message` 只写原因不写业务内容。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushFailure {
    pub code: PushErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushErrorCode {
    /// 凭据未知/签名不符。**不区分**「不认识」与「差一点」,不给枚举预言机。
    Unauthorized,
    BadRequest,
    /// nonce 重放或时间戳超出窗口。
    Replay,
    RateLimited,
    /// 全部目标设备都被 APNs 判定失效。
    TokenRejected,
    /// APNs 端 5xx / 连接失败。
    Upstream,
    /// 发送端自身未就绪(缺 APNs 凭据)。
    Unavailable,
}

/// 自托管 relay 的引导/诊断结果(宿主 `remote.relayDiagnostics`)。
/// 产品方不提供默认 relay(Q10),所以这里必须能回答「你填的 relay 到底能用吗」,
/// 而不是把失败笼统说成「连不上」。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelayDiagnostics {
    pub url: String,
    /// `wss://`(或 loopback 的 `ws://`)。
    pub scheme_ok: bool,
    pub reachable: bool,
    /// relay 接受了控制连接的注册(即口令正确)。
    pub authenticated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// 面向用户的一句话结论(本地化在 Swift 侧,这里是稳定英文诊断)。
    pub detail: String,
}

/// 宿主与发送端共用的签名串。**只有这一份实现**,两侧不各写一遍格式。
///
/// 必须覆盖方法与路径(防把 `/v1/push` 的签名重放到 `/v1/bind`)、时间戳与
/// nonce(防重放),以及请求的**语义字段**(防篡改 payload)。
pub fn signature_string(
    method: &str,
    path: &str,
    issued_at_ms: i64,
    nonce: &str,
    body: &str,
) -> String {
    format!("{method}\n{path}\n{issued_at_ms}\n{nonce}\n{body}")
}

/// 绑定/推送签名共用的方法名(两条路径都是 POST)。
pub const PUSH_METHOD: &str = "POST";
pub const PUSH_BIND_PATH: &str = "/v1/bind";
pub const PUSH_SEND_PATH: &str = "/v1/push";

/// 字段长度前缀的规范化:LINE 形如 `<byte_len>:<value>`,末尾换行。
///
/// 为什么要长度前缀而不是直接拼接:直接拼接时 `("ab","c")` 与 `("a","bc")`
/// 得到同一个串,签名就丢了一个自由度 —— 攻击者可以让服务端解出不同的字段
/// 切分却仍通过校验。长度前缀把字段边界钉死。
fn canonical_field(out: &mut String, value: &str) {
    use std::fmt::Write as _;
    // `write!` 到 `String` 不会失败(只有 fmt 实现返回 Err 才可能),这里没有
    // 可失败的错误源,因此 `let _ =` 是安全的,不是吞异常。
    let _ = writeln!(out, "{}:{}", value.len(), value);
}

/// 手机/宿主签名 `/v1/bind` 的规范化消息。覆盖全部语义字段,因此改 token、
/// 改 platform、换 device 都会让签名失效。
pub fn bind_signature_message(
    user: &str,
    device_id: &str,
    platform: &str,
    token: &str,
    issued_at_ms: i64,
    nonce: &str,
) -> String {
    let mut body = String::new();
    for field in [user, device_id, platform, token] {
        canonical_field(&mut body, field);
    }
    canonical_field(&mut body, &issued_at_ms.to_string());
    canonical_field(&mut body, nonce);
    signature_string(
        PUSH_METHOD,
        PUSH_BIND_PATH,
        issued_at_ms,
        nonce,
        body.trim_end(),
    )
}

/// 宿主签名 `/v1/push` 的规范化消息。`device_ids` **按发送顺序**参与签名:
/// 发送端回执里的 `invalidatedDeviceIds` 依赖同一顺序,签名把顺序也钉住了。
pub fn push_signature_message(
    user: &str,
    device_ids: &[String],
    kind: PushKind,
    event_seq: u64,
    collapse_key: &str,
    issued_at_ms: i64,
    nonce: &str,
) -> String {
    let mut body = String::new();
    canonical_field(&mut body, user);
    canonical_field(&mut body, &device_ids.len().to_string());
    for device_id in device_ids {
        canonical_field(&mut body, device_id);
    }
    canonical_field(&mut body, kind.wire_name());
    canonical_field(&mut body, &event_seq.to_string());
    canonical_field(&mut body, collapse_key);
    canonical_field(&mut body, &issued_at_ms.to_string());
    canonical_field(&mut body, nonce);
    signature_string(
        PUSH_METHOD,
        PUSH_SEND_PATH,
        issued_at_ms,
        nonce,
        body.trim_end(),
    )
}

/// `/v1/push`
pub fn push_send_url(base: &str) -> String {
    format!("{}/v1/push", base.trim_end_matches('/'))
}

/// `/v1/bind`(注册与注销共用;注销靠 `token` 为空字符串表达)
pub fn push_bind_url(base: &str) -> String {
    format!("{}/v1/bind", base.trim_end_matches('/'))
}

/// `/v1/health`
pub fn push_health_url(base: &str) -> String {
    format!("{}/v1/health", base.trim_end_matches('/'))
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

    #[test]
    fn push_envelope_has_no_field_for_business_content() {
        // 载荷白名单是**形状**约束:序列化一个信封后,键集合必须恰好是这五个。
        // 任何「顺手加个 taskTitle 方便调试」的改动都会让本用例变红。
        let envelope = PushEnvelope {
            v: PUSH_PROTOCOL_VERSION,
            kind: PushKind::ApprovalPending,
            event_seq: 42,
            collapse_key: "task-7:approval".to_string(),
        };
        let value: serde_json::Value = serde_json::to_value(&envelope).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["collapseKey", "eventSeq", "kind", "v"]);

        // 三种 kind 的 loc-key 与 collapse tag 都在客户端 bundle 侧,不进网络。
        assert_eq!(PushKind::TaskDone.loc_key(), "push.task_done");
        assert_eq!(PushKind::TaskFailed.loc_key(), "push.task_failed");
        assert_eq!(PushKind::ApprovalPending.loc_key(), "push.approval_pending");
        assert_eq!(PushKind::TaskDone.collapse_tag(), "done");
    }

    #[test]
    fn push_wire_shapes_are_stable_and_round_trip() {
        let request = PushRequest {
            v: PUSH_PROTOCOL_VERSION,
            user: "u1".to_string(),
            device_ids: vec!["d1".to_string()],
            envelope: PushEnvelope {
                v: PUSH_PROTOCOL_VERSION,
                kind: PushKind::TaskFailed,
                event_seq: 9,
                collapse_key: "task-9:failed".to_string(),
            },
            issued_at_ms: 1_700_000_000_000,
            nonce: "n1".to_string(),
            signature: "deadbeef".to_string(),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(
            json.contains(r#""kind":"task_failed""#),
            "kind wire name: {json}"
        );
        assert!(json.contains(r#""deviceIds":["d1"]"#), "device ids: {json}");
        assert_eq!(serde_json::from_str::<PushRequest>(&json).unwrap(), request);

        // 绑定请求只在这里携带 token,且注销用空串表达。
        let bind = PushBindRequest {
            v: PUSH_PROTOCOL_VERSION,
            user: "u1".to_string(),
            device_id: "d1".to_string(),
            platform: "ios".to_string(),
            token: String::new(),
            issued_at_ms: 0,
            nonce: "n2".to_string(),
            signature: "sig".to_string(),
        };
        let bind_json = serde_json::to_string(&bind).unwrap();
        assert!(
            bind_json.contains(r#""token":"""#),
            "unbind wire: {bind_json}"
        );

        let ack = PushAck {
            v: PUSH_PROTOCOL_VERSION,
            accepted: 1,
            invalidated_device_ids: vec!["d2".to_string()],
            retry_after_ms: Some(4_000),
        };
        let ack_json = serde_json::to_string(&ack).unwrap();
        assert!(
            ack_json.contains(r#""invalidatedDeviceIds":["d2"]"#),
            "{ack_json}"
        );
        assert!(ack_json.contains(r#""retryAfterMs":4000"#), "{ack_json}");

        // 无退避要求时不写该键(发送端与宿主都按「缺失=无要求」解析)。
        let plain = PushAck {
            v: PUSH_PROTOCOL_VERSION,
            accepted: 1,
            invalidated_device_ids: Vec::new(),
            retry_after_ms: None,
        };
        assert!(!serde_json::to_string(&plain)
            .unwrap()
            .contains("retryAfterMs"));

        let failure = PushFailure {
            code: PushErrorCode::Replay,
            message: "nonce already used".to_string(),
            retryable: false,
        };
        let failure_json = serde_json::to_string(&failure).unwrap();
        assert!(
            failure_json.contains(r#""code":"replay""#),
            "{failure_json}"
        );
    }

    #[test]
    fn signature_string_covers_method_path_time_nonce_and_full_body() {
        let base = signature_string("POST", "/v1/push", 1, "n", r#"{"a":1}"#);
        assert_eq!(base, "POST\n/v1/push\n1\nn\n{\"a\":1}");
        // 四个可变段各自改变签名:方法、路径(防跨端点重放)、时间/nonce(防重放)、
        // 体(防篡改)。
        let variants = [
            signature_string("GET", "/v1/push", 1, "n", r#"{"a":1}"#),
            signature_string("POST", "/v1/bind", 1, "n", r#"{"a":1}"#),
            signature_string("POST", "/v1/push", 2, "n", r#"{"a":1}"#),
            signature_string("POST", "/v1/push", 1, "m", r#"{"a":1}"#),
            signature_string("POST", "/v1/push", 1, "n", r#"{"a":2}"#),
        ];
        for variant in variants {
            assert_ne!(variant, base);
        }
    }

    #[test]
    fn canonical_field_encoding_is_length_prefixed_so_field_boundaries_cannot_shift() {
        // 长度前缀的作用:直接拼接会让 ("ab","c") 与 ("a","bc") 得到同一个串。
        let a =
            push_signature_message("ab", &["c".to_string()], PushKind::TaskDone, 1, "k", 5, "n");
        let b =
            push_signature_message("a", &["bc".to_string()], PushKind::TaskDone, 1, "k", 5, "n");
        assert_ne!(a, b, "field boundaries must be unambiguous");

        // 钉住整条规范化串的形状(宿主与发送端共同依赖它)。
        let canonical = push_signature_message(
            "owner",
            &["dev-a".to_string(), "dev-b".to_string()],
            PushKind::ApprovalPending,
            42,
            "ab12cd34ef56:approval",
            1_700_000_000_000,
            "nonce-z",
        );
        assert_eq!(
            canonical,
            "POST\n/v1/push\n1700000000000\nnonce-z\n5:owner\n1:2\n5:dev-a\n5:dev-b\n16:approval_pending\n2:42\n21:ab12cd34ef56:approval\n13:1700000000000\n7:nonce-z"
        );

        // 绑定面同理,字段顺序固定为 user/device/platform/token/at/nonce。
        assert_eq!(
            bind_signature_message("owner", "dev-1", "ios", "aabbccdd", 1_700_000_000_000, "bind-nonce"),
            "POST\n/v1/bind\n1700000000000\nbind-nonce\n5:owner\n5:dev-1\n3:ios\n8:aabbccdd\n13:1700000000000\n10:bind-nonce"
        );
    }

    #[test]
    fn push_urls_join_without_double_slashes() {
        assert_eq!(
            push_send_url("https://push.example.com/"),
            "https://push.example.com/v1/push"
        );
        assert_eq!(
            push_bind_url("https://push.example.com"),
            "https://push.example.com/v1/bind"
        );
        assert_eq!(
            push_health_url("http://127.0.0.1:6792/"),
            "http://127.0.0.1:6792/v1/health"
        );
    }
}
