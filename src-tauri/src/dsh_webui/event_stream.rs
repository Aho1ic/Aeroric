//! dsh web 后端的事件下行链路:WebSocket 优先,SSE 兜底。
//!
//! 从 `dsh_webui.rs` 拆出来的一块:SSE 帧切分与信封校验、两种传输各自的
//! 下行 worker、以及把 HTTP base URL 换算成 `ws://` / `wss://` 端点。
//!
//! 这块只碰字节流与 URL,不碰 `AppHandle` / `DshWebUiManager` —— 建链所需的
//! `DshApiClient` 由父模块的 `open_dsh_event_downlink` 传进来,握手失败时的
//! 降级决策也留在那里。

use futures_util::StreamExt;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
use url::Url;

/// 下行事件通道的缓冲深度。
const DSH_EVENT_CHANNEL_CAPACITY: usize = 64;

/// 从缓冲区头部切下一个完整 SSE 帧,并把它连分隔符一起排掉。
///
/// 分隔符定位走 `crate::sse` 那份共用实现 —— 这里原先内联了一份等价的
/// match 展开(全 crate 第五份)。合并前穷举比对过,见那个模块的头注释。
fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let (boundary, delimiter_len) = crate::sse::find_sse_delimiter(buffer)?;
    let frame = buffer[..boundary].to_vec();
    buffer.drain(..boundary + delimiter_len);
    Some(frame)
}

fn parse_sse_envelope(frame: &[u8]) -> Result<Option<Value>, String> {
    let text = std::str::from_utf8(frame)
        .map_err(|error| format!("DSH event frame was not UTF-8: {error}"))?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(None);
    }
    parse_dsh_event_envelope(&data).map(Some)
}

pub(super) fn parse_dsh_event_envelope(text: &str) -> Result<Value, String> {
    let envelope: Value = serde_json::from_str(text)
        .map_err(|error| format!("DSH event frame was invalid JSON: {error}"))?;
    let valid = envelope.get("type").and_then(Value::as_str) == Some("server-request")
        && envelope
            .get("rpcId")
            .and_then(Value::as_str)
            .is_some_and(|rpc_id| !rpc_id.is_empty())
        && envelope
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|method| !method.is_empty())
        && envelope.get("payload").is_some();
    if !valid {
        return Err("DSH event frame was not a valid server-request envelope".to_string());
    }
    Ok(envelope)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DshEventTransport {
    WebSocket,
    LegacySse,
}

impl DshEventTransport {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::WebSocket => "WebSocket",
            Self::LegacySse => "legacy SSE",
        }
    }
}

/// A generation-scoped DSH event downlink. Dropping it aborts the transport
/// worker immediately, which closes either the WebSocket or legacy SSE body.
pub(super) struct DshEventDownlink {
    pub(super) transport: DshEventTransport,
    pub(super) receiver: mpsc::Receiver<Result<Value, String>>,
    worker: JoinHandle<()>,
}

impl DshEventDownlink {
    pub(super) async fn next(&mut self) -> Option<Result<Value, String>> {
        self.receiver.recv().await
    }
}

impl Drop for DshEventDownlink {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub(super) fn dsh_event_websocket_url(base_url: &str, path: &str) -> Result<Url, String> {
    let mut url = Url::parse(base_url)
        .map_err(|error| format!("Invalid DSH event base URL {base_url:?}: {error}"))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => return Err(format!("Unsupported DSH event URL scheme {other:?}")),
    };
    url.set_scheme(scheme)
        .map_err(|_| format!("Could not convert DSH event URL to {scheme}"))?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(super) fn websocket_downlink(
    mut socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> DshEventDownlink {
    let (sender, receiver) = mpsc::channel(DSH_EVENT_CHANNEL_CAPACITY);
    let worker = tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            let envelope = match message {
                Ok(Message::Text(text)) => parse_dsh_event_envelope(text.as_ref()),
                Ok(Message::Binary(_)) => {
                    Err("DSH event downlink returned an unexpected binary frame".to_string())
                }
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
                Err(error) => Err(format!("DSH WebSocket event stream failed: {error}")),
            };
            let failed = envelope.is_err();
            if sender.send(envelope).await.is_err() || failed {
                break;
            }
        }
    });
    DshEventDownlink {
        transport: DshEventTransport::WebSocket,
        receiver,
        worker,
    }
}

pub(super) fn legacy_sse_downlink(response: reqwest::Response) -> DshEventDownlink {
    let (sender, receiver) = mpsc::channel(DSH_EVENT_CHANNEL_CAPACITY);
    let worker = tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let _ = sender
                        .send(Err(format!("DSH SSE event stream failed: {error}")))
                        .await;
                    return;
                }
            };
            buffer.extend_from_slice(&chunk);
            while let Some(frame) = take_sse_frame(&mut buffer) {
                match parse_sse_envelope(&frame) {
                    Ok(Some(envelope)) => {
                        if sender.send(Ok(envelope)).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = sender.send(Err(error)).await;
                        return;
                    }
                }
            }
        }
    });
    DshEventDownlink {
        transport: DshEventTransport::LegacySse,
        receiver,
        worker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_dsh_event_urls_to_websocket_endpoints() {
        assert_eq!(
            dsh_event_websocket_url("http://127.0.0.1:43127", "/api/events.mux")
                .expect("HTTP endpoint converts")
                .as_str(),
            "ws://127.0.0.1:43127/api/events.mux"
        );
        assert_eq!(
            dsh_event_websocket_url("https://example.test/base?old=1", "/api/events.host")
                .expect("HTTPS endpoint converts")
                .as_str(),
            "wss://example.test/api/events.host"
        );
    }

    #[test]
    fn accepts_only_valid_server_request_event_envelopes() {
        let parsed = parse_dsh_event_envelope(
            r#"{"type":"server-request","rpcId":"rpc-1","method":"session/subscribed","payload":{"type":"session/subscribed"}}"#,
        )
        .expect("valid server-request parses");
        assert_eq!(parsed["method"], "session/subscribed");

        assert!(parse_dsh_event_envelope("not json")
            .expect_err("invalid JSON is rejected")
            .contains("invalid JSON"));
        assert!(parse_dsh_event_envelope(
            r#"{"type":"client-request","rpcId":"rpc-1","method":"session/subscribed","payload":{}}"#,
        )
        .expect_err("wrong envelope direction is rejected")
        .contains("server-request"));
    }
}
