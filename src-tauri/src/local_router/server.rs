use super::usage::{self, TokenUsage, UsageCapture, UsageStore};
use super::{
    RouterAgent, RouterRequestRecord, RouterRuntimeConfig, RuntimeMetrics, UpstreamTarget,
    HEALTH_PATH, ROUTE_AGENT_HEADER,
};
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::header::{CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, HeaderName, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use futures_util::stream::{self, BoxStream};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::io;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use url::Url;

const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024 * 1024;
const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

#[derive(Clone)]
pub(crate) struct ServerContext {
    pub(crate) client: reqwest::Client,
    pub(crate) config: Arc<RwLock<RouterRuntimeConfig>>,
    pub(crate) usage_store: UsageStore,
    pub(crate) metrics: Arc<RuntimeMetrics>,
}

pub(crate) fn router(context: ServerContext) -> Router {
    Router::new()
        .route(HEALTH_PATH, get(health_check))
        .fallback(any(proxy_request))
        .with_state(context)
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

async fn proxy_request(State(context): State<ServerContext>, request: Request) -> Response {
    let started = Instant::now();
    let started_at = usage::unix_millis();
    let (parts, body) = request.into_parts();
    let route = match select_route(&parts.uri, &parts.headers) {
        Ok(route) => route,
        Err(message) => return json_error(StatusCode::NOT_FOUND, message),
    };

    let runtime_config = context.config.read().await.clone();
    let record_usage = runtime_config.record_usage;
    let target = runtime_config.upstreams.target(route.agent).cloned();
    let mut completion = RequestCompletion::new(
        route.agent,
        started,
        started_at,
        record_usage,
        context.usage_store.clone(),
        context.metrics.clone(),
    );

    let body_bytes = match to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            completion
                .complete(
                    TokenUsage::default(),
                    StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
                    false,
                    Some(
                        "local router rejected an oversized or unreadable request body".to_string(),
                    ),
                )
                .await;
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body exceeds the local router limit",
            );
        }
    };

    let request_metadata = request_metadata(&body_bytes);
    completion.set_request_model(request_metadata.model.clone());
    completion.set_streaming(request_metadata.streaming);

    let Some(target) = target else {
        completion
            .complete(
                TokenUsage::default(),
                StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                request_metadata.streaming,
                Some("routing is disabled for this agent".to_string()),
            )
            .await;
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "local routing is disabled for this agent",
        );
    };

    let upstream_url = match build_upstream_url(&target, &route.forward_path, parts.uri.query()) {
        Ok(url) => url,
        Err(message) => {
            completion
                .complete(
                    TokenUsage::default(),
                    StatusCode::BAD_GATEWAY.as_u16(),
                    request_metadata.streaming,
                    Some(message.to_string()),
                )
                .await;
            return json_error(StatusCode::BAD_GATEWAY, "invalid upstream route");
        }
    };

    let method = parts.method.clone();
    let outbound_headers = filter_request_headers(parts.headers);
    let upstream_response = context
        .client
        .request(method.clone(), upstream_url)
        .headers(outbound_headers)
        .body(body_bytes)
        .send()
        .await;
    let upstream_response = match upstream_response {
        Ok(response) => response,
        Err(error) => {
            let (status, summary) = classify_send_error(&error);
            completion
                .complete(
                    TokenUsage::default(),
                    status.as_u16(),
                    request_metadata.streaming,
                    Some(summary.to_string()),
                )
                .await;
            return json_error(status, summary);
        }
    };

    let status = upstream_response.status();
    let response_headers = filter_response_headers(upstream_response.headers().clone());
    let is_sse = response_headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false);
    let is_streaming = request_metadata.streaming || is_sse;
    let content_encoding = response_headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok());
    let content_encoding = content_encoding.map(str::to_string);
    let response_error =
        (status.as_u16() >= 400).then(|| format!("upstream returned HTTP {}", status.as_u16()));
    let expected_body_bytes = upstream_response.content_length();

    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;

    if method == axum::http::Method::HEAD
        || matches!(status, StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED)
    {
        completion
            .complete(
                TokenUsage::default(),
                status.as_u16(),
                is_streaming,
                response_error.clone(),
            )
            .await;
        return response;
    }

    let state = ProxyBodyState {
        upstream: Box::pin(upstream_response.bytes_stream()),
        capture: Some(UsageCapture::new(
            route.agent,
            is_streaming,
            content_encoding.as_deref(),
        )),
        completion: Some(completion),
        status_code: status.as_u16(),
        is_streaming,
        response_error,
        expected_body_bytes,
        forwarded_body_bytes: 0,
    };
    let stream = stream::unfold(Some(state), |state| async move {
        let Some(mut state) = state else {
            return None;
        };
        match state.upstream.next().await {
            Some(Ok(chunk)) => {
                if let Some(capture) = state.capture.as_mut() {
                    capture.push(&chunk);
                }
                state.forwarded_body_bytes = state
                    .forwarded_body_bytes
                    .saturating_add(chunk.len() as u64);
                if state.expected_body_bytes == Some(state.forwarded_body_bytes) {
                    state.finish(None).await;
                    Some((Ok::<Bytes, io::Error>(chunk), None))
                } else {
                    Some((Ok::<Bytes, io::Error>(chunk), Some(state)))
                }
            }
            Some(Err(error)) => {
                let summary = classify_body_error(&error);
                state.finish(Some(&summary)).await;
                Some((Err(io::Error::other(summary)), None))
            }
            None => {
                state.finish(None).await;
                None
            }
        }
    });
    *response.body_mut() = Body::from_stream(stream);
    response
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectedRoute {
    agent: RouterAgent,
    forward_path: String,
}

fn select_route(uri: &Uri, headers: &HeaderMap) -> Result<SelectedRoute, &'static str> {
    let marker = match headers.get(ROUTE_AGENT_HEADER) {
        Some(value) => {
            let value = value
                .to_str()
                .map_err(|_| "invalid local router agent marker")?;
            Some(match value.trim().to_ascii_lowercase().as_str() {
                "claude" => RouterAgent::Claude,
                "codex" => RouterAgent::Codex,
                _ => return Err("invalid local router agent marker"),
            })
        }
        None => None,
    };

    let path = uri.path();
    let prefixed = strip_agent_prefix(path, "/claude", RouterAgent::Claude)
        .or_else(|| strip_agent_prefix(path, "/codex", RouterAgent::Codex));
    let (agent, forward_path) = if let Some((agent, path)) = prefixed {
        if marker.is_some_and(|marker| marker != agent) {
            return Err("local router path and agent marker disagree");
        }
        (agent, path)
    } else if path.starts_with("/v1/messages") {
        (RouterAgent::Claude, path.to_string())
    } else if path.starts_with("/v1/responses")
        || path.starts_with("/responses")
        || path.starts_with("/v1/chat/completions")
        || path.starts_with("/chat/completions")
    {
        (RouterAgent::Codex, path.to_string())
    } else if let Some(agent) = marker {
        (agent, path.to_string())
    } else {
        return Err("unknown local router endpoint");
    };

    let forward_path = if agent == RouterAgent::Codex {
        normalize_codex_path(&forward_path)
    } else {
        forward_path
    };
    Ok(SelectedRoute {
        agent,
        forward_path,
    })
}

fn strip_agent_prefix(
    path: &str,
    prefix: &str,
    agent: RouterAgent,
) -> Option<(RouterAgent, String)> {
    let suffix = path.strip_prefix(prefix)?;
    if !suffix.is_empty() && !suffix.starts_with('/') {
        return None;
    }
    Some((
        agent,
        if suffix.is_empty() {
            "/".to_string()
        } else {
            suffix.to_string()
        },
    ))
}

fn normalize_codex_path(path: &str) -> String {
    path.strip_prefix("/v1/v1/")
        .map(|suffix| format!("/v1/{suffix}"))
        .unwrap_or_else(|| path.to_string())
}

fn build_upstream_url(
    target: &UpstreamTarget,
    request_path: &str,
    query: Option<&str>,
) -> Result<Url, &'static str> {
    let mut url = target.base_url().clone();
    let base_segments = url
        .path()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let mut request_segments = request_path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if target
        .base_url()
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("chatgpt.com"))
        && target
            .base_url()
            .path()
            .trim_end_matches('/')
            .ends_with("/backend-api/codex")
        && request_segments.first() == Some(&"v1")
    {
        request_segments.remove(0);
    }

    let maximum_overlap = base_segments.len().min(request_segments.len());
    let overlap = (0..=maximum_overlap)
        .rev()
        .find(|count| {
            base_segments[base_segments.len().saturating_sub(*count)..]
                == request_segments[..*count]
        })
        .unwrap_or(0);
    let mut combined = base_segments;
    combined.extend_from_slice(&request_segments[overlap..]);
    let joined_path = if combined.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", combined.join("/"))
    };
    url.set_path(&joined_path);
    url.set_query(query);
    Ok(url)
}

fn filter_request_headers(headers: HeaderMap) -> HeaderMap {
    let mut filtered = filter_hop_by_hop(headers);
    filtered.remove(HOST);
    filtered.remove(CONTENT_LENGTH);
    filtered.remove(ROUTE_AGENT_HEADER);
    filtered
}

fn filter_response_headers(headers: HeaderMap) -> HeaderMap {
    filter_hop_by_hop(headers)
}

fn filter_hop_by_hop(mut headers: HeaderMap) -> HeaderMap {
    let connection_headers = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| HeaderName::from_bytes(value.as_bytes()).ok())
        .collect::<Vec<_>>();
    for header in HOP_BY_HOP_HEADERS {
        headers.remove(*header);
    }
    for header in connection_headers {
        headers.remove(header);
    }
    headers
}

#[derive(Default)]
struct RequestMetadata {
    model: String,
    streaming: bool,
}

fn request_metadata(body: &[u8]) -> RequestMetadata {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return RequestMetadata::default();
    };
    RequestMetadata {
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .unwrap_or_default()
            .to_string(),
        streaming: value
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

struct CompletionContext {
    request_id: String,
    agent: RouterAgent,
    request_model: String,
    started: Instant,
    started_at: i64,
    record_usage: bool,
    is_streaming: bool,
    usage_store: UsageStore,
    metrics: Arc<RuntimeMetrics>,
}

struct RequestCompletion {
    context: Option<CompletionContext>,
}

impl RequestCompletion {
    fn new(
        agent: RouterAgent,
        started: Instant,
        started_at: i64,
        record_usage: bool,
        usage_store: UsageStore,
        metrics: Arc<RuntimeMetrics>,
    ) -> Self {
        metrics.begin_request();
        Self {
            context: Some(CompletionContext {
                request_id: uuid::Uuid::new_v4().to_string(),
                agent,
                request_model: String::new(),
                started,
                started_at,
                record_usage,
                is_streaming: false,
                usage_store,
                metrics,
            }),
        }
    }

    fn set_request_model(&mut self, model: String) {
        if let Some(context) = self.context.as_mut() {
            context.request_model = model;
        }
    }

    fn set_streaming(&mut self, streaming: bool) {
        if let Some(context) = self.context.as_mut() {
            context.is_streaming = streaming;
        }
    }

    async fn complete(
        mut self,
        usage: TokenUsage,
        status_code: u16,
        is_streaming: bool,
        error_summary: Option<String>,
    ) {
        if let Some(mut context) = self.context.take() {
            context.is_streaming = is_streaming;
            finalize_request(context, usage, status_code, error_summary).await;
        }
    }

    fn complete_detached(
        mut self,
        usage: TokenUsage,
        status_code: u16,
        is_streaming: bool,
        error_summary: String,
    ) {
        let Some(mut context) = self.context.take() else {
            return;
        };
        context.is_streaming = is_streaming;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(finalize_request(
                context,
                usage,
                status_code,
                Some(error_summary),
            ));
        } else {
            context.metrics.finish_request(false);
            context
                .metrics
                .set_error(Some(context.agent), &error_summary);
        }
    }
}

impl Drop for RequestCompletion {
    fn drop(&mut self) {
        let Some(context) = self.context.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(finalize_request(
                context,
                TokenUsage::default(),
                0,
                Some("local router request was cancelled".to_string()),
            ));
        } else {
            context.metrics.finish_request(false);
            context
                .metrics
                .set_error(Some(context.agent), "local router request was cancelled");
        }
    }
}

async fn finalize_request(
    context: CompletionContext,
    usage: TokenUsage,
    status_code: u16,
    error_summary: Option<String>,
) {
    let success = error_summary.is_none() && (200..400).contains(&status_code);
    context.metrics.finish_request(success);
    let error_summary = error_summary.as_deref().map(usage::sanitize_summary);
    if let Some(message) = error_summary.as_deref() {
        context.metrics.set_error(Some(context.agent), message);
    }
    if !context.record_usage {
        return;
    }

    let completed_at = usage::unix_millis();
    let model = usage
        .model
        .clone()
        .filter(|model| !model.trim().is_empty())
        .or_else(|| {
            (!context.request_model.trim().is_empty()).then_some(context.request_model.clone())
        })
        .unwrap_or_else(|| "unknown".to_string());
    let record = RouterRequestRecord {
        request_id: context.request_id,
        response_id: usage.response_id,
        agent: context.agent,
        model,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        status_code,
        latency_ms: context.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        started_at: context.started_at,
        completed_at,
        is_streaming: context.is_streaming,
        success,
        error_summary,
    };
    if let Err(error) = context.usage_store.insert(record).await {
        context.metrics.set_error(
            Some(context.agent),
            format!("failed to record local router usage: {error}"),
        );
    }
}

struct ProxyBodyState {
    upstream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    capture: Option<UsageCapture>,
    completion: Option<RequestCompletion>,
    status_code: u16,
    is_streaming: bool,
    response_error: Option<String>,
    expected_body_bytes: Option<u64>,
    forwarded_body_bytes: u64,
}

impl ProxyBodyState {
    async fn finish(mut self, transport_error: Option<&str>) {
        let usage = self
            .capture
            .take()
            .map(UsageCapture::finish)
            .unwrap_or_default();
        let error = transport_error
            .map(str::to_string)
            .or_else(|| self.response_error.clone());
        if let Some(completion) = self.completion.take() {
            completion
                .complete(usage, self.status_code, self.is_streaming, error)
                .await;
        }
    }
}

impl Drop for ProxyBodyState {
    fn drop(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let usage = self
            .capture
            .take()
            .map(UsageCapture::finish)
            .unwrap_or_default();
        let error = self.response_error.take().unwrap_or_else(|| {
            "client disconnected before the upstream response completed".to_string()
        });
        completion.complete_detached(usage, self.status_code, self.is_streaming, error);
    }
}

fn classify_send_error(error: &reqwest::Error) -> (StatusCode, &'static str) {
    if error.is_timeout() {
        (StatusCode::GATEWAY_TIMEOUT, "upstream request timed out")
    } else if error.is_connect() {
        (StatusCode::BAD_GATEWAY, "failed to connect to upstream")
    } else {
        (StatusCode::BAD_GATEWAY, "failed to send upstream request")
    }
}

fn classify_body_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "upstream response stream timed out"
    } else {
        "upstream response stream failed"
    }
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "type": "local_router_error",
                "message": message,
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_router::{
        LocalRouterState, RouterUpstreams, UpstreamTarget, ROUTE_AGENT_HEADER,
    };
    use axum::http::HeaderValue;
    use std::convert::Infallible;
    use std::fs;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[test]
    fn agent_prefixes_and_compatibility_paths_are_distinct() {
        let request = Request::builder()
            .uri("/claude/v1/messages")
            .body(Body::empty())
            .unwrap();
        let route = select_route(request.uri(), request.headers()).unwrap();
        assert_eq!(route.agent, RouterAgent::Claude);
        assert_eq!(route.forward_path, "/v1/messages");

        let request = Request::builder()
            .uri("/codex/v1/responses")
            .body(Body::empty())
            .unwrap();
        let route = select_route(request.uri(), request.headers()).unwrap();
        assert_eq!(route.agent, RouterAgent::Codex);
        assert_eq!(route.forward_path, "/v1/responses");

        let request = Request::builder()
            .uri("/v1/responses")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            select_route(request.uri(), request.headers())
                .unwrap()
                .agent,
            RouterAgent::Codex
        );
    }

    #[test]
    fn upstream_join_avoids_duplicate_version_segments() {
        let target = UpstreamTarget::new("https://example.com/api/v1").unwrap();
        let url = build_upstream_url(&target, "/v1/responses", Some("trace=1")).unwrap();
        assert_eq!(url.as_str(), "https://example.com/api/v1/responses?trace=1");

        let target = UpstreamTarget::new("https://example.com").unwrap();
        let url = build_upstream_url(&target, "/v1/messages", None).unwrap();
        assert_eq!(url.as_str(), "https://example.com/v1/messages");
    }

    #[test]
    fn chatgpt_codex_upstream_drops_the_openai_v1_segment() {
        let target = UpstreamTarget::new("https://chatgpt.com/backend-api/codex").unwrap();
        let url = build_upstream_url(&target, "/v1/responses/compact", None).unwrap();
        assert_eq!(
            url.as_str(),
            "https://chatgpt.com/backend-api/codex/responses/compact"
        );
    }

    #[test]
    fn request_filter_preserves_auth_and_removes_transport_and_internal_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));
        headers.insert("x-api-key", HeaderValue::from_static("secret"));
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive, x-remove"));
        headers.insert("x-remove", HeaderValue::from_static("private"));
        headers.insert(ROUTE_AGENT_HEADER, HeaderValue::from_static("codex"));
        headers.insert(HOST, HeaderValue::from_static("127.0.0.1"));
        let filtered = filter_request_headers(headers);
        assert_eq!(filtered["authorization"], "Bearer secret");
        assert_eq!(filtered["x-api-key"], "secret");
        assert!(!filtered.contains_key(CONNECTION));
        assert!(!filtered.contains_key("x-remove"));
        assert!(!filtered.contains_key(ROUTE_AGENT_HEADER));
        assert!(!filtered.contains_key(HOST));
    }

    #[derive(Clone, Debug, Default)]
    struct CapturedRequest {
        uri: String,
        authorization: Option<String>,
        internal_marker_present: bool,
        removed_header_present: bool,
    }

    fn unused_port() -> u16 {
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn temp_database_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "aeroric-router-integration-{}.sqlite3",
            Uuid::new_v4()
        ))
    }

    fn remove_database(path: &PathBuf) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(PathBuf::from(format!("{}{suffix}", path.display())));
        }
    }

    async fn start_mock_upstream(app: Router) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (address, task)
    }

    #[tokio::test]
    async fn forwards_json_and_records_usage_without_sensitive_request_data() {
        let captured = Arc::new(Mutex::new(CapturedRequest::default()));
        let mock_state = captured.clone();
        let upstream = Router::new().fallback(any(move |request: Request| {
            let state = mock_state.clone();
            async move {
                let mut captured = state.lock().await;
                captured.uri = request.uri().to_string();
                captured.authorization = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                captured.internal_marker_present =
                    request.headers().contains_key(ROUTE_AGENT_HEADER);
                captured.removed_header_present = request.headers().contains_key("x-remove");
                Json(json!({
                    "id": "resp_json_1",
                    "model": "gpt-5.6",
                    "usage": {
                        "input_tokens": 21,
                        "output_tokens": 8,
                        "input_tokens_details": {"cached_tokens": 5}
                    },
                    "output": []
                }))
            }
        }));
        let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;

        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let local_port = unused_port();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                local_port,
                true,
                RouterUpstreams {
                    claude: None,
                    codex: Some(
                        UpstreamTarget::new(format!("http://{upstream_address}/api/v1")).unwrap(),
                    ),
                },
            ))
            .await
            .unwrap();

        let response = reqwest::Client::new()
            .post(format!("{}/codex/v1/responses?trace=1", info.base_url))
            .header("authorization", "Bearer test-secret")
            .header(ROUTE_AGENT_HEADER, "codex")
            .header("connection", "x-remove")
            .header("x-remove", "must-not-forward")
            .json(&json!({"model": "gpt-5.6", "input": "private prompt"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response_json = response.json::<Value>().await.unwrap();
        assert_eq!(response_json["id"], "resp_json_1");

        let captured = captured.lock().await.clone();
        assert_eq!(captured.uri, "/api/v1/responses?trace=1");
        assert_eq!(
            captured.authorization.as_deref(),
            Some("Bearer test-secret")
        );
        assert!(!captured.internal_marker_present);
        assert!(!captured.removed_header_present);

        let recent = router.recent_requests(10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].input_tokens, 21);
        assert_eq!(recent[0].output_tokens, 8);
        assert_eq!(recent[0].cache_read_tokens, 5);
        assert_eq!(recent[0].response_id.as_deref(), Some("resp_json_1"));
        assert!(recent[0].success);
        assert_eq!(recent[0].error_summary, None);
        router.stop().await.unwrap();
        upstream_task.abort();
        let _ = upstream_task.await;
        remove_database(&database_path);
    }

    #[tokio::test]
    async fn forwards_sse_chunks_and_records_terminal_usage() {
        let first = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream_1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":13,\"output_tokens\":0,\"cache_read_input_tokens\":2}}}\n\n";
        let second = "event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\n";
        let expected = format!("{first}{second}");
        let upstream = Router::new().fallback(any(move || {
            let chunks = vec![
                Ok::<Bytes, Infallible>(Bytes::copy_from_slice(&first.as_bytes()[..41])),
                Ok::<Bytes, Infallible>(Bytes::copy_from_slice(&first.as_bytes()[41..])),
                Ok::<Bytes, Infallible>(Bytes::from_static(second.as_bytes())),
            ];
            async move {
                let mut response = Response::new(Body::from_stream(stream::iter(chunks)));
                response
                    .headers_mut()
                    .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
                response
            }
        }));
        let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;

        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                RouterUpstreams {
                    claude: Some(
                        UpstreamTarget::new(format!("http://{upstream_address}")).unwrap(),
                    ),
                    codex: None,
                },
            ))
            .await
            .unwrap();
        let response = reqwest::Client::new()
            .post(format!("{}/claude/v1/messages", info.base_url))
            .json(&json!({"model": "claude-sonnet-4-5", "stream": true}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), expected);

        let recent = router.recent_requests(10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].input_tokens, 13);
        assert_eq!(recent[0].output_tokens, 7);
        assert_eq!(recent[0].cache_read_tokens, 2);
        assert_eq!(recent[0].response_id.as_deref(), Some("msg_stream_1"));
        assert!(recent[0].is_streaming);

        router.stop().await.unwrap();
        upstream_task.abort();
        let _ = upstream_task.await;
        remove_database(&database_path);
    }
}
