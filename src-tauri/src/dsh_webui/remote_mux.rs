//! Bidirectional client for the Gateway Remote-stream mux (`/api/remote.mux`).
//!
//! Replaces the retired `/api/events.mux` / `/api/events.host` firehose
//! (zero hits at pin `c291e7961a`). The physical socket is one WebSocket; every
//! Typert Remote stream (`session/follow`, `session/control`, `workspace/follow`,
//! `$events`, …) is a logical channel multiplexed on it.
//!
//! Wire contract is `packages/api/gateway/src/stream-protocol.ts` at that pin:
//!
//! - uplink: `{type:'open', streamId, endpoint, payload}` or `{type:'cancel', streamId}`
//!   (exact keys; `payload` is always `{args: <plain object>}`)
//! - downlink: `{type:'item', streamId, value?}` / `{type:'end', streamId}` /
//!   `{type:'error', streamId, error:{code, message, details}}` (exact keys)
//!
//! There is no SSE fallback: the carrier is bidirectional, and SSE cannot
//! carry an `open` frame. A failed handshake is a hard error.

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use url::Url;
use uuid::Uuid;

/// Exact WebSocket route carrying every Typert Remote stream.
pub(super) const REMOTE_STREAM_MUX_PATH: &str = "/api/remote.mux";

/// Gateway-internal logical stream carrying application-selected Cordis events.
pub(super) const REMOTE_EVENT_STREAM_ENDPOINT: &str = "$events";

/// Gateway-internal unary endpoint returning one Client Remote Event outcome.
pub(super) const REMOTE_EVENT_RESULT_ENDPOINT: &str = "$events/result";

const STREAM_CHANNEL_CAPACITY: usize = 64;
const COMMAND_CHANNEL_CAPACITY: usize = 32;
/// Flush auto-generated Ping replies at least this often so the Host heartbeat
/// (default 2s, terminate after 2 missed pongs) does not kill an idle socket.
const PONG_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// One physical `/api/remote.mux` generation. Dropping it aborts the worker,
/// which closes the socket; every live [`DshMuxStream`] then yields `None`.
pub(super) struct DshRemoteMux {
    commands: mpsc::Sender<MuxCommand>,
    streams: Arc<Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>>,
    alive: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

enum MuxCommand {
    Open {
        stream_id: String,
        endpoint: String,
        payload: Value,
    },
    Cancel {
        stream_id: String,
    },
}

/// One independently cancellable logical stream on a [`DshRemoteMux`].
///
/// Dropping the stream sends `{type:'cancel', streamId}` if the Host has not
/// already ended it. `next` yields each `item.value` (`Null` when omitted),
/// `Err` on an `error` frame, and `None` on `end` or carrier loss.
pub(super) struct DshMuxStream {
    stream_id: String,
    receiver: mpsc::Receiver<Result<Value, String>>,
    commands: mpsc::Sender<MuxCommand>,
    streams: Arc<Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>>,
    ended: bool,
}

impl DshRemoteMux {
    /// Connect to `{base}/api/remote.mux` and wait for the WebSocket handshake.
    pub(super) async fn connect(base_url: &str) -> Result<Self, String> {
        let url = remote_mux_url(base_url)?;
        let (socket, _) = connect_async(url.as_str())
            .await
            .map_err(|error| format!("DSH remote.mux WebSocket handshake failed: {error}"))?;
        Ok(Self::from_socket(socket))
    }

    fn from_socket(socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>) -> Self {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let streams = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let worker_streams = streams.clone();
        let worker_alive = alive.clone();
        let worker = tokio::spawn(async move {
            run_mux_worker(socket, command_rx, worker_streams).await;
            worker_alive.store(false, Ordering::SeqCst);
        });
        Self {
            commands: command_tx,
            streams,
            alive,
            worker,
        }
    }

    pub(super) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Open one logical stream. `args` is the inner args object (possibly empty);
    /// the mux wraps it as `{args}` to match Gateway `remoteRequest`.
    pub(super) fn open(&self, endpoint: &str, args: Value) -> Result<DshMuxStream, String> {
        if endpoint.is_empty() {
            return Err("DSH remote.mux endpoint must be non-empty".to_string());
        }
        if !self.is_alive() {
            return Err("DSH remote.mux carrier is closed".to_string());
        }
        let args = match args {
            Value::Object(_) => args,
            _ => return Err("DSH remote.mux payload args must be a plain object".to_string()),
        };
        let stream_id = Uuid::new_v4().to_string();
        let (item_tx, item_rx) = mpsc::channel(STREAM_CHANNEL_CAPACITY);
        self.streams.lock().insert(stream_id.clone(), item_tx);
        let payload = json!({ "args": args });
        self.commands
            .try_send(MuxCommand::Open {
                stream_id: stream_id.clone(),
                endpoint: endpoint.to_string(),
                payload,
            })
            .map_err(|_| {
                self.streams.lock().remove(&stream_id);
                "DSH remote.mux carrier is closed".to_string()
            })?;
        Ok(DshMuxStream {
            stream_id,
            receiver: item_rx,
            commands: self.commands.clone(),
            streams: self.streams.clone(),
            ended: false,
        })
    }
}

impl Drop for DshRemoteMux {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.worker.abort();
    }
}

impl DshMuxStream {
    pub(super) async fn next(&mut self) -> Option<Result<Value, String>> {
        if self.ended {
            return None;
        }
        match self.receiver.recv().await {
            Some(Ok(value)) => Some(Ok(value)),
            Some(Err(error)) => {
                self.ended = true;
                Some(Err(error))
            }
            None => {
                self.ended = true;
                None
            }
        }
    }
}

impl Drop for DshMuxStream {
    fn drop(&mut self) {
        self.streams.lock().remove(&self.stream_id);
        if !self.ended {
            let _ = self.commands.try_send(MuxCommand::Cancel {
                stream_id: self.stream_id.clone(),
            });
        }
    }
}

async fn run_mux_worker(
    mut socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    mut commands: mpsc::Receiver<MuxCommand>,
    streams: Arc<Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>>,
) {
    let mut flush_tick = interval(PONG_FLUSH_INTERVAL);
    flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(MuxCommand::Open { stream_id, endpoint, payload }) => {
                        let text = encode_open(&stream_id, &endpoint, payload);
                        if socket.send(Message::Text(text)).await.is_err() {
                            fail_all(&streams, "DSH remote.mux send failed");
                            return;
                        }
                    }
                    Some(MuxCommand::Cancel { stream_id }) => {
                        let text = encode_cancel(&stream_id);
                        if socket.send(Message::Text(text)).await.is_err() {
                            fail_all(&streams, "DSH remote.mux send failed");
                            return;
                        }
                    }
                    None => {
                        let _ = socket.close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: "disposed".into(),
                        })).await;
                        fail_all(&streams, "DSH remote.mux client disposed");
                        return;
                    }
                }
            }
            inbound = socket.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        match parse_server_message(&text) {
                            Ok(ServerMessage::Item { stream_id, value }) => {
                                let sender = streams.lock().get(&stream_id).cloned();
                                if let Some(sender) = sender {
                                    if sender.send(Ok(value)).await.is_err() {
                                        streams.lock().remove(&stream_id);
                                    }
                                }
                            }
                            Ok(ServerMessage::End { stream_id }) => {
                                streams.lock().remove(&stream_id);
                            }
                            Ok(ServerMessage::Error { stream_id, error }) => {
                                // Bind first: a `parking_lot` guard held by `if let`
                                // would span the await and make the future non-Send.
                                let sender = streams.lock().remove(&stream_id);
                                if let Some(sender) = sender {
                                    let _ = sender.send(Err(error)).await;
                                }
                            }
                            Err(error) => {
                                fail_all(&streams, &error);
                                let _ = socket.close(Some(CloseFrame {
                                    code: CloseCode::Library(4002),
                                    reason: "invalid Remote stream frame".into(),
                                })).await;
                                return;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {
                        let _ = socket.flush().await;
                    }
                    Some(Ok(Message::Frame(_))) => {}
                    Some(Ok(Message::Binary(_))) => {
                        fail_all(&streams, "DSH remote.mux requires text messages");
                        let _ = socket.close(Some(CloseFrame {
                            code: CloseCode::Unsupported,
                            reason: "text messages required".into(),
                        })).await;
                        return;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        fail_all(&streams, "DSH remote.mux WebSocket closed");
                        return;
                    }
                    Some(Err(error)) => {
                        fail_all(&streams, &format!("DSH remote.mux WebSocket failed: {error}"));
                        return;
                    }
                }
            }
            _ = flush_tick.tick() => {
                let _ = socket.flush().await;
            }
        }
    }
}

fn fail_all(streams: &Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>, error: &str) {
    let pending: Vec<_> = streams.lock().drain().map(|(_, sender)| sender).collect();
    for sender in pending {
        let _ = sender.try_send(Err(error.to_string()));
    }
}

pub(super) fn remote_mux_url(base_url: &str) -> Result<Url, String> {
    let mut url = Url::parse(base_url)
        .map_err(|error| format!("Invalid DSH remote.mux base URL {base_url:?}: {error}"))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => return Err(format!("Unsupported DSH remote.mux URL scheme {other:?}")),
    };
    url.set_scheme(scheme)
        .map_err(|_| format!("Could not convert DSH remote.mux URL to {scheme}"))?;
    url.set_path(REMOTE_STREAM_MUX_PATH);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn encode_open(stream_id: &str, endpoint: &str, payload: Value) -> String {
    json!({
        "type": "open",
        "streamId": stream_id,
        "endpoint": endpoint,
        "payload": payload,
    })
    .to_string()
}

fn encode_cancel(stream_id: &str) -> String {
    json!({
        "type": "cancel",
        "streamId": stream_id,
    })
    .to_string()
}

#[derive(Debug)]
enum ServerMessage {
    Item { stream_id: String, value: Value },
    End { stream_id: String },
    Error { stream_id: String, error: String },
}

/// Parse one Host-to-client text frame. Extra or missing keys are a protocol
/// error (the official parser uses `exactKeys`); a bad frame fails the
/// physical generation, matching `RemoteStreamMuxClient.receive`.
fn parse_server_message(text: &str) -> Result<ServerMessage, String> {
    let decoded: Value = serde_json::from_str(text)
        .map_err(|error| format!("DSH remote.mux frame is not JSON: {error}"))?;
    let object = as_plain_object(&decoded)
        .ok_or_else(|| "DSH remote.mux frame must be an object".to_string())?;
    let frame_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "DSH remote.mux frame is missing type".to_string())?;
    match frame_type {
        "item" => {
            require_exact_keys(object, &["type", "streamId"], &["value"])?;
            let stream_id = require_id(object, "streamId")?;
            let value = object.get("value").cloned().unwrap_or(Value::Null);
            Ok(ServerMessage::Item { stream_id, value })
        }
        "end" => {
            require_exact_keys(object, &["type", "streamId"], &[])?;
            Ok(ServerMessage::End {
                stream_id: require_id(object, "streamId")?,
            })
        }
        "error" => {
            require_exact_keys(object, &["type", "streamId", "error"], &[])?;
            let stream_id = require_id(object, "streamId")?;
            let error = parse_stream_failure(object.get("error").unwrap_or(&Value::Null))?;
            Ok(ServerMessage::Error { stream_id, error })
        }
        other => Err(format!("DSH remote.mux frame has unknown type {other:?}")),
    }
}

fn parse_stream_failure(value: &Value) -> Result<String, String> {
    let object = as_plain_object(value)
        .ok_or_else(|| "DSH remote.mux error must be an object".to_string())?;
    require_exact_keys(object, &["code", "message", "details"], &[])?;
    let code = object
        .get("code")
        .and_then(Value::as_str)
        .ok_or_else(|| "DSH remote.mux error code must be a string".to_string())?;
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| "DSH remote.mux error message must be a string".to_string())?;
    if !object
        .get("details")
        .is_some_and(|details| details.is_object())
    {
        return Err("DSH remote.mux error details must be an object".to_string());
    }
    Ok(format!("{code}: {message}"))
}

fn as_plain_object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn require_id(object: &Map<String, Value>, key: &str) -> Result<String, String> {
    match object.get(key).and_then(Value::as_str) {
        Some(id) if !id.is_empty() => Ok(id.to_string()),
        _ => Err(format!("DSH remote.mux {key} must be a non-empty string")),
    }
}

fn require_exact_keys(
    object: &Map<String, Value>,
    required: &[&str],
    optional: &[&str],
) -> Result<(), String> {
    for key in required {
        if !object.contains_key(*key) {
            return Err(format!("DSH remote.mux frame is missing {key}"));
        }
    }
    for key in object.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(format!("DSH remote.mux frame has unexpected key {key}"));
        }
    }
    Ok(())
}

/// Decode one `$events` item. The opening ready frame is accepted only as the
/// first item; later frames must be emit / waterfall / cancel with exact keys.
pub(super) fn parse_remote_event_item(value: &Value) -> Result<RemoteEventItem, String> {
    let object =
        as_plain_object(value).ok_or_else(|| "DSH $events item must be an object".to_string())?;
    let event_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "DSH $events item is missing type".to_string())?;
    match event_type {
        "ready" => {
            require_exact_keys(object, &["type", "clientId", "host"], &[])?;
            let client_id = require_id(object, "clientId")?;
            let host = object
                .get("host")
                .and_then(as_plain_object)
                .ok_or_else(|| "DSH $events ready.host must be an object".to_string())?;
            require_exact_keys(host, &["home"], &[])?;
            let home = host
                .get("home")
                .and_then(Value::as_str)
                .ok_or_else(|| "DSH $events ready.host.home must be a string".to_string())?
                .to_string();
            Ok(RemoteEventItem::Ready { client_id, home })
        }
        "emit" => {
            require_exact_keys(object, &["type", "event", "args"], &[])?;
            let event = require_id(object, "event")?;
            let args = object
                .get("args")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| "DSH $events emit.args must be an array".to_string())?;
            Ok(RemoteEventItem::Emit { event, args })
        }
        "waterfall" => {
            require_exact_keys(
                object,
                &["type", "event", "eventId", "agentId", "request"],
                &[],
            )?;
            let event = require_id(object, "event")?;
            let event_id = require_id(object, "eventId")?;
            let agent_id = require_id(object, "agentId")?;
            let request = object
                .get("request")
                .cloned()
                .filter(Value::is_object)
                .ok_or_else(|| "DSH $events waterfall.request must be an object".to_string())?;
            Ok(RemoteEventItem::Waterfall {
                event,
                event_id,
                agent_id,
                request,
            })
        }
        "cancel" => {
            require_exact_keys(object, &["type", "eventId"], &[])?;
            Ok(RemoteEventItem::Cancel {
                event_id: require_id(object, "eventId")?,
            })
        }
        other => Err(format!("DSH $events item has unknown type {other:?}")),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum RemoteEventItem {
    Ready {
        client_id: String,
        home: String,
    },
    Emit {
        event: String,
        args: Vec<Value>,
    },
    Waterfall {
        event: String,
        event_id: String,
        agent_id: String,
        request: Value,
    },
    Cancel {
        event_id: String,
    },
}

/// POST `/api/$events/result` args object. The unary envelope is applied by
/// [`crate::dsh_webui::DshApiClient::remote_call`].
pub(super) fn remote_event_result_args(client_id: &str, event_id: &str, outcome: Value) -> Value {
    json!({
        "clientId": client_id,
        "eventId": event_id,
        "outcome": outcome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::SinkExt;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    #[test]
    fn converts_http_base_urls_to_the_mux_websocket() {
        assert_eq!(
            remote_mux_url("http://127.0.0.1:43127")
                .expect("HTTP endpoint converts")
                .as_str(),
            "ws://127.0.0.1:43127/api/remote.mux"
        );
        assert_eq!(
            remote_mux_url("https://example.test/base?old=1")
                .expect("HTTPS endpoint converts")
                .as_str(),
            "wss://example.test/api/remote.mux"
        );
    }

    #[test]
    fn rejects_unsupported_url_schemes() {
        let error = remote_mux_url("ftp://example.test").expect_err("ftp is rejected");
        assert!(error.contains("scheme"));
    }

    #[test]
    fn parses_item_end_and_error_with_exact_keys() {
        let item =
            parse_server_message(r#"{"type":"item","streamId":"s1","value":{"type":"baseline"}}"#)
                .expect("item parses");
        match item {
            ServerMessage::Item { stream_id, value } => {
                assert_eq!(stream_id, "s1");
                assert_eq!(value["type"], "baseline");
            }
            _ => panic!("expected item"),
        }

        let item_without_value =
            parse_server_message(r#"{"type":"item","streamId":"s1"}"#).expect("item parses");
        match item_without_value {
            ServerMessage::Item { value, .. } => assert!(value.is_null()),
            _ => panic!("expected item"),
        }

        let end = parse_server_message(r#"{"type":"end","streamId":"s1"}"#).expect("end parses");
        match end {
            ServerMessage::End { stream_id } => assert_eq!(stream_id, "s1"),
            _ => panic!("expected end"),
        }

        let error = parse_server_message(
            r#"{"type":"error","streamId":"s1","error":{"code":"gateway/internal","message":"boom","details":{}}}"#,
        )
        .expect("error parses");
        match error {
            ServerMessage::Error { stream_id, error } => {
                assert_eq!(stream_id, "s1");
                assert_eq!(error, "gateway/internal: boom");
            }
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn rejects_malformed_server_frames() {
        for (frame, needle) in [
            ("not json", "not JSON"),
            ("[]", "object"),
            (r#"{"type":"item"}"#, "streamId"),
            (r#"{"type":"item","streamId":""}"#, "non-empty"),
            (
                r#"{"type":"item","streamId":"s1","extra":1}"#,
                "unexpected key",
            ),
            (
                r#"{"type":"end","streamId":"s1","value":1}"#,
                "unexpected key",
            ),
            (
                r#"{"type":"error","streamId":"s1","error":{"code":"x","message":"y"}}"#,
                "details",
            ),
            (r#"{"type":"nope","streamId":"s1"}"#, "unknown type"),
        ] {
            let error = parse_server_message(frame).expect_err(frame);
            assert!(
                error.contains(needle),
                "frame {frame:?} error {error:?} should contain {needle:?}"
            );
        }
    }

    #[test]
    fn open_and_cancel_frames_use_exact_keys() {
        // `parseRemoteStreamClientMessage` checks the key *set* with `exactKeys`,
        // so the assertion sorts rather than pinning serde_json's insertion order.
        let sorted_keys = |value: &Value| {
            let mut keys = value
                .as_object()
                .expect("frame is an object")
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            keys.sort();
            keys
        };

        let open: Value = serde_json::from_str(&encode_open(
            "s1",
            "workspace/follow",
            json!({ "args": {} }),
        ))
        .expect("open encodes");
        assert_eq!(
            sorted_keys(&open),
            vec!["endpoint", "payload", "streamId", "type"]
        );
        assert_eq!(open["type"], "open");
        assert_eq!(open["streamId"], "s1");
        assert_eq!(open["endpoint"], "workspace/follow");
        assert_eq!(open["payload"], json!({ "args": {} }));

        let cancel: Value = serde_json::from_str(&encode_cancel("s1")).expect("cancel encodes");
        assert_eq!(sorted_keys(&cancel), vec!["streamId", "type"]);
        assert_eq!(cancel["type"], "cancel");
        assert_eq!(cancel["streamId"], "s1");
    }

    #[test]
    fn parses_events_ready_emit_waterfall_and_cancel() {
        let ready = parse_remote_event_item(&json!({
            "type": "ready",
            "clientId": "c1",
            "host": { "home": "/Users/me" },
        }))
        .expect("ready parses");
        assert_eq!(
            ready,
            RemoteEventItem::Ready {
                client_id: "c1".into(),
                home: "/Users/me".into(),
            }
        );

        let emit = parse_remote_event_item(&json!({
            "type": "emit",
            "event": "api-session/added",
            "args": [{ "sessionId": "s1" }],
        }))
        .expect("emit parses");
        match emit {
            RemoteEventItem::Emit { event, args } => {
                assert_eq!(event, "api-session/added");
                assert_eq!(args.len(), 1);
            }
            _ => panic!("expected emit"),
        }

        let waterfall = parse_remote_event_item(&json!({
            "type": "waterfall",
            "event": "approval/request",
            "eventId": "e1",
            "agentId": "s1",
            "request": { "toolName": "Bash" },
        }))
        .expect("waterfall parses");
        match waterfall {
            RemoteEventItem::Waterfall {
                event,
                event_id,
                agent_id,
                request,
            } => {
                assert_eq!(event, "approval/request");
                assert_eq!(event_id, "e1");
                assert_eq!(agent_id, "s1");
                assert_eq!(request["toolName"], "Bash");
            }
            _ => panic!("expected waterfall"),
        }

        let cancel = parse_remote_event_item(&json!({
            "type": "cancel",
            "eventId": "e1",
        }))
        .expect("cancel parses");
        assert_eq!(
            cancel,
            RemoteEventItem::Cancel {
                event_id: "e1".into()
            }
        );
    }

    #[test]
    fn rejects_malformed_events_items() {
        let extra_ready = parse_remote_event_item(&json!({
            "type": "ready",
            "clientId": "c1",
            "host": { "home": "/x" },
            "extra": 1,
        }))
        .expect_err("extra key");
        assert!(extra_ready.contains("unexpected key"));

        let empty_args = parse_remote_event_item(&json!({
            "type": "emit",
            "event": "api-session/added",
            "args": {},
        }))
        .expect_err("args must be an array");
        assert!(empty_args.contains("args"));
    }

    async fn accept_one(listener: &TcpListener) -> WebSocketStream<tokio::net::TcpStream> {
        let (stream, _) = listener.accept().await.expect("client connects");
        tokio_tungstenite::accept_async(stream)
            .await
            .expect("handshake succeeds")
    }

    async fn recv_text(socket: &mut WebSocketStream<tokio::net::TcpStream>) -> Value {
        loop {
            match socket.next().await {
                Some(Ok(WsMessage::Text(text))) => {
                    return serde_json::from_str(&text).expect("client frame is JSON")
                }
                Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_))) => continue,
                other => panic!("expected text frame, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn opens_a_logical_stream_and_delivers_items_until_end() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let mut socket = accept_one(&listener).await;
            let open = recv_text(&mut socket).await;
            assert_eq!(open["type"], "open");
            assert_eq!(open["endpoint"], "workspace/follow");
            assert_eq!(open["payload"], json!({ "args": {} }));
            let stream_id = open["streamId"].as_str().expect("streamId").to_string();
            socket
                .send(WsMessage::Text(
                    json!({
                        "type": "item",
                        "streamId": stream_id,
                        "value": { "type": "baseline", "value": { "items": [], "archivedSessionIds": [] } },
                    })
                    .to_string(),
                ))
                .await
                .expect("item sent");
            socket
                .send(WsMessage::Text(
                    json!({ "type": "end", "streamId": stream_id }).to_string(),
                ))
                .await
                .expect("end sent");
            socket.close(None).await.ok();
        });

        let mux = DshRemoteMux::connect(&format!("http://{address}"))
            .await
            .expect("mux connects");
        let mut stream = mux
            .open("workspace/follow", json!({}))
            .expect("stream opens");
        let item = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("item arrives")
            .expect("stream stays open")
            .expect("item is ok");
        assert_eq!(item["type"], "baseline");
        let end = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("end arrives");
        assert!(end.is_none());
        server.await.expect("server exits");
    }

    #[tokio::test]
    async fn routes_frames_by_stream_id() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let mut socket = accept_one(&listener).await;
            let first = recv_text(&mut socket).await;
            let second = recv_text(&mut socket).await;
            let (follow, control) = if first["endpoint"] == "session/follow" {
                (first, second)
            } else {
                (second, first)
            };
            let follow_id = follow["streamId"].as_str().unwrap().to_string();
            let control_id = control["streamId"].as_str().unwrap().to_string();
            socket
                .send(WsMessage::Text(
                    json!({
                        "type": "item",
                        "streamId": control_id,
                        "value": { "type": "baseline", "value": { "queues": {}, "jobs": {}, "projections": {} } },
                    })
                    .to_string(),
                ))
                .await
                .expect("control item");
            socket
                .send(WsMessage::Text(
                    json!({
                        "type": "item",
                        "streamId": follow_id,
                        "value": { "type": "snapshot", "cursor": 3 },
                    })
                    .to_string(),
                ))
                .await
                .expect("follow item");
            socket.close(None).await.ok();
        });

        let mux = DshRemoteMux::connect(&format!("http://{address}"))
            .await
            .expect("mux connects");
        let mut follow = mux
            .open(
                "session/follow",
                json!({ "request": { "address": { "kind": "session", "sessionId": "s1" } } }),
            )
            .expect("follow opens");
        let mut control = mux
            .open("session/control", json!({}))
            .expect("control opens");
        let control_item = tokio::time::timeout(Duration::from_secs(1), control.next())
            .await
            .expect("control item")
            .unwrap()
            .unwrap();
        let follow_item = tokio::time::timeout(Duration::from_secs(1), follow.next())
            .await
            .expect("follow item")
            .unwrap()
            .unwrap();
        assert_eq!(control_item["type"], "baseline");
        assert_eq!(follow_item["type"], "snapshot");
        assert_eq!(follow_item["cursor"], 3);
        server.await.expect("server exits");
    }

    #[tokio::test]
    async fn dropping_a_stream_sends_cancel() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let mut socket = accept_one(&listener).await;
            let open = recv_text(&mut socket).await;
            assert_eq!(open["type"], "open");
            let stream_id = open["streamId"].as_str().unwrap().to_string();
            let cancel = recv_text(&mut socket).await;
            assert_eq!(cancel["type"], "cancel");
            assert_eq!(cancel["streamId"], stream_id);
        });

        let mux = DshRemoteMux::connect(&format!("http://{address}"))
            .await
            .expect("mux connects");
        let stream = mux.open("$events", json!({})).expect("stream opens");
        drop(stream);
        server.await.expect("server saw cancel");
    }

    #[tokio::test]
    async fn invalid_frame_fails_every_open_stream() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let mut socket = accept_one(&listener).await;
            let _open = recv_text(&mut socket).await;
            socket
                .send(WsMessage::Text("not a mux frame".to_string()))
                .await
                .expect("invalid frame sent");
            // The client closes the socket; drain until close.
            while let Some(Ok(_)) = socket.next().await {}
        });

        let mux = DshRemoteMux::connect(&format!("http://{address}"))
            .await
            .expect("mux connects");
        let mut stream = mux
            .open("workspace/follow", json!({}))
            .expect("stream opens");
        let result = tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("failure arrives")
            .expect("error is delivered");
        assert!(result
            .expect_err("invalid frame is an error")
            .contains("not JSON"));
        server.await.expect("server exits");
    }

    #[tokio::test]
    async fn error_frame_ends_only_the_named_stream() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let mut socket = accept_one(&listener).await;
            let first = recv_text(&mut socket).await;
            let second = recv_text(&mut socket).await;
            let (a, b) = (first, second);
            socket
                .send(WsMessage::Text(
                    json!({
                        "type": "error",
                        "streamId": a["streamId"],
                        "error": { "code": "gateway/arguments-invalid", "message": "bad args", "details": {} },
                    })
                    .to_string(),
                ))
                .await
                .expect("error sent");
            socket
                .send(WsMessage::Text(
                    json!({
                        "type": "item",
                        "streamId": b["streamId"],
                        "value": { "ok": true },
                    })
                    .to_string(),
                ))
                .await
                .expect("item sent");
            socket.close(None).await.ok();
        });

        let mux = DshRemoteMux::connect(&format!("http://{address}"))
            .await
            .expect("mux connects");
        let mut a = mux
            .open("session/follow", json!({ "request": {} }))
            .unwrap();
        let mut b = mux.open("session/control", json!({})).unwrap();
        let a_result = tokio::time::timeout(Duration::from_secs(1), a.next())
            .await
            .expect("a ends")
            .unwrap();
        let b_result = tokio::time::timeout(Duration::from_secs(1), b.next())
            .await
            .expect("b item")
            .unwrap()
            .unwrap();
        assert!(a_result
            .expect_err("named stream errors")
            .contains("gateway/arguments-invalid"));
        assert_eq!(b_result["ok"], true);
        server.await.expect("server exits");
    }
}
