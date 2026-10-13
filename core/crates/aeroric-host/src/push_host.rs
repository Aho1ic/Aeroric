//! 宿主侧推送出口与自托管 relay 诊断(MIGRATION_PLAN §D AP6)。
//!
//! core 只做决策(载荷最小化、要不要推、退避抑制),真正的网络两件事归宿主:
//!
//! 1. **推送出口**:把 `/v1/bind` 与 `/v1/push` 的 JSON 体 POST 给用户自托管的
//!    发送端(`PushTransport` 端口的实现)。
//! 2. **relay 诊断**:回答「我填的这个 relay 到底能不能用」——产品方不提供
//!    默认 relay(Q10),所以必须能给出可读的结论,而不是笼统的「连不上」。
//!
//! ## 为什么用阻塞 reqwest + 专用 runtime
//!
//! `RemoteHostHandle::push_register` 是 async,但 `PushTransport::send` 刻意是
//! 同步的:它与 `EventLog::append` 在同一个调用栈上(core 的观察者在 append 的
//! 调用者线程被调用),在那里 `.await` 会把**任务结算**拖住等一次网络往返。
//! 同步端口 + `block_in_place` 让「等网络」只占用当前工作线程,Tokio 会把其他
//! 任务调度到别的 worker;宿主进程的资源是宽松的,而任务结算不能被通知拖慢。
//!
//! 这也是为什么本模块用独立 runtime 驱动 reqwest 的阻塞客户端:发送端是
//! **异步** trait 方法(`async_trait`),在那里 `block_on` 会 panic,而
//! `block_in_place` 允许把阻塞工作移出 async 上下文。

use std::sync::Arc;
use std::time::{Duration, Instant};

use remote_protocol::{HostToRelay, RelayDiagnostics, RelayToHost, RELAY_PROTOCOL_VERSION};
use serde_json::{json, Value};

use aeroric_core::remote::push::{PushDiagnostic, PushOutcome, PushSenderClient};
use aeroric_core::remote::push_plane::{parse_sender_response, PushTransport};

/// 单次发送端请求的超时。通知路径不该长时间占着线程。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// relay 诊断:注册握手超时(与 relay 自己的 REGISTER_TIMEOUT 同量级)。
const RELAY_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// 进程级专用 runtime。
///
/// **为什么是进程级单例而不是结构体字段**:拥有一个 `Runtime` 意味着在 `Drop`
/// 时要关停它,而「在 async 上下文里 drop 一个 runtime」会**panic**
/// (`Cannot drop a runtime in a context where blocking is not allowed`)。宿主
/// 停止服务、重载配置或丢弃 handle 都发生在 async 上下文里,所以把 runtime 放在
/// 会被随手 drop 的结构体里等于给宿主编了一个关机即崩的定时炸弹。
/// 放进 `OnceLock` 之后它随进程存活,没有人会去 drop 它。
fn push_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("aeroric-push-http")
            .build()
            .unwrap_or_else(|error| {
                // 建不出 runtime 是启动期配置错误;这里没有优雅退路,但崩溃要给
                // 出可读原因而不是 backtrace。
                panic!("cannot start the push HTTP runtime: {error}")
            })
    })
}

/// 进程级阻塞客户端,且**按需**在安全上下文里构造。
///
/// 两件事都必须在这里处理,否则就是「关机/首推即崩」:
/// 1. `reqwest::blocking::Client` 在 `build()` 时会自建并**丢弃**一个临时
///    runtime;这个丢弃发生在调用线程上,所以 `build()` **不能在 async 上下文里
///    调用**(实测 panic:`Cannot drop a runtime in a context where blocking is
///    not allowed`)。用 `OnceLock` + [`run_blocking`] 把构造挪到安全位置。
/// 2. 客户端必须是进程级的:每次推送新建一个客户端会重复付连接池与 TLS
///    握手的代价,而通知是低频但要求快的路径。
fn blocking_client() -> Result<&'static reqwest::blocking::Client, String> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::blocking::Client, String>> =
        std::sync::OnceLock::new();
    let entry = CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // 发送端是用户自托管的;不跟重定向(把签名带去别处没有意义)。
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("cannot build the push HTTP client: {error}"))
    });
    match entry {
        Ok(client) => Ok(client),
        Err(error) => Err(error.clone()),
    }
}

/// 在同步上下文里执行一段会阻塞的工作。
///
/// * 不在 runtime 里 → 直接执行。
/// * 在多线程 runtime 里 → `block_in_place` 把当前 worker 让出来。
/// * 在**单线程** runtime 里 → 明确报错。阻塞 `current_thread` 会把整个 runtime
///   卡死(连带通知与任务结算),所以这里宁可失败也不制造死锁。
fn run_blocking<F, T>(what: &str, f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    match tokio::runtime::Handle::try_current() {
        Err(_) => f(),
        Ok(handle) => match handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(f),
            _ => Err(format!(
                "{what} needs a multi-threaded runtime: blocking a current-thread runtime would                  freeze task settlement and notifications alike"
            )),
        },
    }
}

/// 真正的 HTTP 出口。**不持有 runtime 也不持有客户端**(见 [`push_runtime`] /
/// [`blocking_client`]):两者都是进程级、按需构造,这样它在 async 上下文里
/// 被构造或被丢弃都不会出问题。
#[derive(Default)]
pub struct HttpPushTransport;

impl HttpPushTransport {
    /// 构造是**廉价且无副作用**的:没有 runtime、没有客户端、不碰网络。
    /// 这样它可以在任何上下文里被调用(包括 async 里),真正的重活推迟到第一次
    /// 投递时在 [`run_blocking`] 里做。
    pub fn new() -> Result<Self, String> {
        install_tls_provider();
        Ok(Self)
    }

    /// 发一次带签名的请求,返回 (HTTP 状态, 响应体)。
    ///
    /// 只负责传输:两种端点(`/v1/bind` 与 `/v1/push`)的回执**形状不同**
    /// (bind 回 `{bound, changed}`,push 回 `PushAck`),所以解析留给各自调用方,
    /// 不在这里统一按 push 的形状解析 —— 否则一次成功的绑定会被误判成
    /// 「unreadable ack」。
    fn post_raw(
        &self,
        client: &PushSenderClient,
        path: &str,
        body: &str,
    ) -> Result<(u16, String), String> {
        let http = blocking_client()?;
        let url = format!("{}{path}", client.config().url);
        let response = http
            .post(&url)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|error| {
                format!(
                    "cannot reach the push sender at {}: {error}",
                    client.config().url
                )
            })?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|error| format!("unreadable response from the push sender: {error}"))?;
        Ok((status, text))
    }

    /// `/v1/push`:`PushAck` 形状,交给 core 的解析器。
    fn post(&self, client: &PushSenderClient, path: &str, body: &str) -> Result<Value, String> {
        let (status, text) = self.post_raw(client, path, body)?;
        parse_sender_response(status, &text)
    }

    /// 注册/注销一台设备的 APNs token。
    fn post_bind(
        &self,
        client: &PushSenderClient,
        params: &Value,
        now_ms: i64,
        nonce: &str,
    ) -> Result<Value, String> {
        let device_id = params
            .get("deviceId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Missing param: deviceId".to_string())?;
        let platform = params
            .get("platform")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("ios");
        let token = params
            .get("token")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        let request = client.build_bind_request(device_id, platform, token, now_ms, nonce);
        let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let (status, text) = self.post_raw(client, remote_protocol::PUSH_BIND_PATH, &body)?;
        if (200..300).contains(&status) {
            return serde_json::from_str::<Value>(&text).map_err(|error| {
                format!("the push sender returned an unreadable bind ack: {error}")
            });
        }
        // 非 2xx 一律走发送端的诊断形状(它把失败原因写在那里)。
        parse_sender_response(status, &text)
    }
}

impl PushTransport for HttpPushTransport {
    fn bind(
        &self,
        client: &PushSenderClient,
        params: &Value,
        now_ms: i64,
        nonce: &str,
    ) -> Result<Value, String> {
        run_blocking("push registration", || {
            self.post_bind(client, params, now_ms, nonce)
        })
    }

    fn send(
        &self,
        client: &PushSenderClient,
        device_ids: &[String],
        decision: &aeroric_core::remote::push::PushDecision,
    ) -> Result<PushOutcome, PushDiagnostic> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        // nonce 用「设备数 + 单调时间」拼:同一毫秒内的并发调用也不会撞。
        let nonce = format!("{now_ms}-{}", decision.envelope.event_seq);
        let request = client.build_push_request(device_ids, &decision.envelope, now_ms, &nonce);
        let body = match serde_json::to_string(&request) {
            Ok(body) => body,
            Err(error) => {
                return Err(PushDiagnostic {
                    code: remote_protocol::PushErrorCode::BadRequest,
                    message: error.to_string(),
                    retryable: false,
                })
            }
        };
        // 阻塞工作交给 `run_blocking`:多线程 runtime 下用 `block_in_place`
        // 让出 worker,单线程下明确报错而不是把 runtime 卡死。
        let result = run_blocking("push delivery", || {
            self.post(client, remote_protocol::PUSH_SEND_PATH, &body)
        });
        match result {
            Ok(value) => Ok(outcome_from_ack(&value)),
            Err(message) => Err(PushDiagnostic {
                // 连接层失败一律算可重试(网络抖动);4xx 的语义已经在发送端
                // 的诊断里表达过,但那条路径不会走到这里。
                code: remote_protocol::PushErrorCode::Upstream,
                message,
                retryable: true,
            }),
        }
    }
}

/// 把发送端回执的 JSON(经 `parse_sender_response`)还原成 core 的 `PushOutcome`。
fn outcome_from_ack(value: &Value) -> PushOutcome {
    PushOutcome {
        accepted: value.get("accepted").and_then(Value::as_u64).unwrap_or(0) as usize,
        invalidated_device_ids: value
            .get("invalidatedDeviceIds")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        retry_after_ms: value.get("retryAfterMs").and_then(Value::as_u64),
    }
}

/// 自托管 relay 的连通性与鉴权探测。
///
/// 只做一件事:像桌面那样连上 `/host`、发 `Register`、等 `Registered`。
/// 成功 = URL 方案对 + 可达 + 口令正确;任何一步失败都给出**具体**原因。
/// 它不建立数据连接,也不改变服务状态。
pub fn probe_relay(relay_url: &str, relay_token: Option<&str>) -> RelayDiagnostics {
    if relay_url.trim().is_empty() {
        return RelayDiagnostics {
            url: String::new(),
            scheme_ok: true,
            reachable: false,
            authenticated: false,
            latency_ms: None,
            detail: "no relay configured; direct LAN connection is the only path".to_string(),
        };
    }
    // 用 core 的纯逻辑判方案(与生产启动同一条规则),这样诊断结论与
    // 「服务到底会不会用它」一致。
    if let Some(problem) = aeroric_core::remote::push::relay_scheme_problem(relay_url) {
        return RelayDiagnostics {
            url: relay_url.to_string(),
            scheme_ok: false,
            reachable: false,
            authenticated: false,
            latency_ms: None,
            detail: problem,
        };
    }
    let started = Instant::now();
    let result = probe_relay_blocking(relay_url, relay_token);
    let latency = started.elapsed().as_millis() as u64;
    match result {
        Ok(detail) => RelayDiagnostics {
            url: relay_url.to_string(),
            scheme_ok: true,
            reachable: true,
            authenticated: true,
            latency_ms: Some(latency),
            detail,
        },
        Err((reachable, detail)) => RelayDiagnostics {
            url: relay_url.to_string(),
            scheme_ok: true,
            reachable,
            authenticated: false,
            latency_ms: Some(latency),
            detail,
        },
    }
}

/// 装配进程级 rustls provider,且只装一次。
///
/// **这不是可选的卫生习惯**:本仓的依赖图里 `hyper-rustls` 带 `ring`,而
/// `reqwest`/`rustls` 的默认 feature 带 `aws-lc-rs`,两者同时启用时 rustls
/// **在首次使用 TLS 时 panic**,而不是自己选一个。推送出口与 relay 诊断都在
/// 这里做 TLS,所以 provider 必须在第一次连接前显式装好。
pub fn install_tls_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // 已经有人装过(例如宿主其它 TLS 面)时 `install_default` 返回 Err,
        // 那说明环境已经可用,忽略即可。
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// 真发一次 WS 注册。返回 `Err((reachable, detail))`。
///
/// 整段(包括驱动它的 runtime)走 [`run_blocking`]:探测会被 async 调用方触发,
/// 在那里阻塞 current_thread runtime 或 drop 一个 runtime 都会 panic/死锁。
fn probe_relay_blocking(
    relay_url: &str,
    relay_token: Option<&str>,
) -> Result<String, (bool, String)> {
    install_tls_provider();
    // `run_blocking` 的错误面是 `String`(它只表达「不能在这里阻塞」),而探测
    // 自己的错误面是 `(reachable, detail)`。这里把前者映射成「不可达」,因为
    // 「没法探测」对用户而言就是「这条 relay 现在用不了」。
    match run_blocking("relay diagnostics", || {
        Ok(push_runtime().block_on(probe_relay_async(
            relay_url.to_string(),
            relay_token.map(str::to_string),
        )))
    }) {
        Ok(result) => result,
        Err(blocking_error) => Err((false, blocking_error)),
    }
}

async fn probe_relay_async(
    relay_url: String,
    relay_token: Option<String>,
) -> Result<String, (bool, String)> {
    use futures_util::{SinkExt, StreamExt};

    let endpoint = remote_protocol::host_control_url(&relay_url);
    let connect = tokio_tungstenite::connect_async(&endpoint);
    let stream = match tokio::time::timeout(RELAY_PROBE_TIMEOUT, connect).await {
        Ok(Ok((stream, _))) => stream,
        Ok(Err(error)) => {
            return Err((false, format!("cannot connect to {endpoint}: {error}")));
        }
        Err(_) => {
            return Err((
                false,
                format!("timed out connecting to {endpoint} ({RELAY_PROBE_TIMEOUT:?})"),
            ));
        }
    };
    let (mut sink, mut source) = stream.split();

    // relay 需要 hostId;诊断用一个一次性的探测 id,不与真实 host 抢注册位。
    let host_id = "aeroric-diagnostic-probe";
    let register = HostToRelay::Register {
        v: RELAY_PROTOCOL_VERSION,
        host_id: host_id.to_string(),
        token: relay_token,
    };
    let payload = match serde_json::to_string(&register) {
        Ok(payload) => payload,
        Err(error) => return Err((true, format!("cannot encode the register frame: {error}"))),
    };
    if let Err(error) = sink
        .send(tokio_tungstenite::tungstenite::Message::Text(payload))
        .await
    {
        return Err((true, format!("cannot send the register frame: {error}")));
    }

    match tokio::time::timeout(RELAY_PROBE_TIMEOUT, source.next()).await {
        Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) => {
            match serde_json::from_str::<RelayToHost>(&text) {
                Ok(RelayToHost::Registered) => Ok(format!(
                    "relay accepted the registration for {host_id}; phones can reach this host through it"
                )),
                Ok(RelayToHost::Error { message }) => {
                    Err((true, format!("relay refused the registration: {message}")))
                }
                Ok(RelayToHost::ClientConnected { .. }) => Err((
                    true,
                    "relay answered with an unexpected control message".to_string(),
                )),
                Err(error) => Err((
                    true,
                    format!("relay's control message was unreadable: {error}"),
                )),
            }
        }
        Ok(Some(Ok(other))) => Err((
            true,
            format!("relay answered with an unexpected frame: {other:?}"),
        )),
        Ok(Some(Err(error))) => Err((true, format!("relay connection failed: {error}"))),
        Ok(None) => Err((true, "relay closed the connection before answering".to_string())),
        Err(_) => Err((
            true,
            format!(
                "relay did not answer the registration within {RELAY_PROBE_TIMEOUT:?} \
                 (reachable, but the token may be rejected or the proxy may not upgrade)"
            ),
        )),
    }
}

/// 诊断响应的 JSON 形状(供 `remote.relayDiagnostics` 直接使用)。
pub fn relay_diagnostics_value(diagnostics: &RelayDiagnostics) -> Value {
    serde_json::to_value(diagnostics).unwrap_or_else(|_| json!({"url": diagnostics.url}))
}

/// 本机可作为配对角色的候选直连地址(诊断里一并回给用户,便于对比
/// 「relay 不通时直连是否可行」)。
pub fn direct_endpoints_hint() -> Value {
    json!({
        "loopbackOnly": false,
        "note": "direct connections are suitable for LAN or a trusted overlay network (Tailscale, WireGuard); \
                 non-loopback relay URLs must use wss://",
    })
}

/// 让宿主在启停推送时复用同一个 runtime(避免每条通知新建)。
pub fn shared_transport() -> Result<Arc<dyn PushTransport>, String> {
    HttpPushTransport::new().map(|transport| Arc::new(transport) as Arc<dyn PushTransport>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_from_ack_reads_the_wire_fields() {
        let outcome = outcome_from_ack(&json!({
            "accepted": 2,
            "invalidatedDeviceIds": ["d1", 7],
            "retryAfterMs": 3000
        }));
        assert_eq!(outcome.accepted, 2);
        assert_eq!(outcome.invalidated_device_ids, vec!["d1".to_string()]);
        assert_eq!(outcome.retry_after_ms, Some(3000));

        // 缺字段全部按缺省处理,不 panic。
        let sparse = outcome_from_ack(&json!({}));
        assert_eq!(sparse.accepted, 0);
        assert!(sparse.invalidated_device_ids.is_empty());
        assert_eq!(sparse.retry_after_ms, None);
    }

    #[test]
    fn relay_probe_rejects_bad_schemes_before_touching_the_network() {
        let http = probe_relay("http://relay.example.com", Some("t"));
        assert!(!http.scheme_ok);
        assert!(!http.reachable);
        assert!(http.detail.contains("wss://"), "{}", http.detail);

        let bare_ws = probe_relay("ws://relay.example.com", Some("t"));
        assert!(!bare_ws.scheme_ok, "non-loopback ws:// must be refused");

        // 空 URL:方案没问题,但没有 relay 可用。
        let empty = probe_relay("", None);
        assert!(empty.scheme_ok);
        assert!(!empty.reachable);
        assert!(
            empty.detail.contains("no relay configured"),
            "{}",
            empty.detail
        );
    }

    #[test]
    fn relay_probe_reports_unreachable_instead_of_pretending_success() {
        // 127.0.0.1 上一个几乎肯定没人监听的端口:方案合法 → 走进网络探测 → 失败。
        let diagnostics = probe_relay("ws://127.0.0.1:1", Some("token"));
        assert!(diagnostics.scheme_ok);
        assert!(!diagnostics.authenticated);
        assert!(
            diagnostics.detail.contains("cannot connect")
                || diagnostics.detail.contains("timed out"),
            "{}",
            diagnostics.detail
        );
    }

    #[test]
    fn diagnostics_serialize_with_camel_case_keys() {
        let value = relay_diagnostics_value(&RelayDiagnostics {
            url: "wss://relay.example.com".to_string(),
            scheme_ok: true,
            reachable: true,
            authenticated: true,
            latency_ms: Some(12),
            detail: "ok".to_string(),
        });
        assert_eq!(value["schemeOk"], json!(true));
        assert_eq!(value["latencyMs"], json!(12));
        assert_eq!(value["authenticated"], json!(true));
        // 没有 latency 时不写该键(可选字段)。
        let value = relay_diagnostics_value(&RelayDiagnostics {
            url: "wss://relay.example.com".to_string(),
            scheme_ok: true,
            reachable: false,
            authenticated: false,
            latency_ms: None,
            detail: "no".to_string(),
        });
        assert!(value.get("latencyMs").is_none());
    }

    #[test]
    fn transport_construction_is_available_on_this_host() {
        // 没有 APNs 凭据也能构造(凭据在发送端);构造失败说明运行时/客户端
        // 有问题,那应当在启动时就暴露。
        let transport = shared_transport().expect("transport");
        // 真正发送需要发送端地址,这里只验证端口可用。
        let _: Arc<dyn PushTransport> = transport;
    }
}
