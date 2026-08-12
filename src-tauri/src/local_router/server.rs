use super::cache_injector;
use super::chat_bridge::{chat_response_to_responses, responses_to_chat, ChatSseTransformer};
use super::circuit_breaker::{CircuitBreakerConfig, CircuitBreakerRegistry, CircuitPermit};
use super::session;
use super::thinking_optimizer;
use super::transforms::{self, PreparedRequest};
use super::usage::{self, TokenUsage, UsageCapture, UsageStore};
use super::{
    RouterAgent, RouterAgentPolicy, RouterRequestRecord, RouterRuntimeConfig, RuntimeMetrics,
    UpstreamTarget, HEALTH_PATH, ROUTER_TOKEN_HEADER, ROUTE_AGENT_HEADER,
};
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::header::{
    ACCEPT_ENCODING, AUTHORIZATION, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE,
    HOST,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use futures_util::stream::{self, BoxStream};
use futures_util::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use url::Url;

const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_STREAM_PRIME_BYTES: usize = 256 * 1024;
const ANTHROPIC_ONE_M_BETA: &str = "context-1m-2025-08-07";
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
const NON_RETRYABLE_STATUS_CODES: &[u16] = &[400, 405, 406, 413, 414, 415, 422, 501];

#[derive(Clone)]
pub(crate) struct ServerContext {
    pub(crate) config: Arc<RwLock<RouterRuntimeConfig>>,
    pub(crate) usage_store: UsageStore,
    pub(crate) metrics: Arc<RuntimeMetrics>,
    pub(crate) circuit_breakers: Arc<CircuitBreakerRegistry>,
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
    let (mut parts, body) = request.into_parts();
    let route = match select_route(&parts.uri, &parts.headers) {
        Ok(route) => route,
        Err(message) => return json_error(StatusCode::NOT_FOUND, message),
    };

    let runtime_config = context.config.read().await.clone();
    let agent_runtime = runtime_config.upstreams.agent(route.agent).clone();
    if !request_is_authorized(&runtime_config, route.agent, &parts.headers) {
        return unauthorized_response();
    }
    strip_router_credentials(&mut parts.headers, &runtime_config.access_token);
    let mut completion = RequestCompletion::new(
        route.agent,
        started,
        started_at,
        runtime_config.record_usage,
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
    completion.set_endpoint(route.forward_path.clone());
    let session_body = serde_json::from_slice::<Value>(&body_bytes).ok();
    completion.set_session_id(Some(session::extract_session_id(
        route.agent,
        &parts.headers,
        session_body.as_ref(),
    )));

    let candidates = agent_runtime.candidates();
    if candidates.is_empty() {
        let message = if agent_runtime.policy.auto_failover_enabled {
            "automatic failover is enabled, but its queue has no valid targets"
        } else {
            "routing is disabled or no active target is configured for this agent"
        };
        completion
            .complete(
                TokenUsage::default(),
                StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                request_metadata.streaming,
                Some(message.to_string()),
            )
            .await;
        return json_error(StatusCode::SERVICE_UNAVAILABLE, message);
    }

    let method = parts.method;
    let query = parts.uri.query().map(str::to_string);
    let outbound_headers = filter_request_headers(parts.headers);
    let policy = agent_runtime.policy;
    let circuit_config = policy.circuit_breaker_config();
    let max_attempts = policy.max_attempts();
    let bypass_circuit_breaker = !policy.auto_failover_enabled;
    let mut attempted_targets = 0usize;
    let mut last_failure: Option<AttemptFailure> = None;
    let mut saw_circuit_candidate = false;

    for target in candidates {
        if attempted_targets >= max_attempts {
            break;
        }

        let permit = if bypass_circuit_breaker {
            CircuitPermit {
                allowed: true,
                used_half_open_permit: false,
            }
        } else {
            context
                .circuit_breakers
                .allow_request(route.agent, target.id(), circuit_config.clone())
                .await
        };
        if !permit.allowed {
            continue;
        }
        saw_circuit_candidate = true;
        attempted_targets += 1;

        let attempt = attempt_target(
            &runtime_config,
            &route,
            &method,
            query.as_deref(),
            &outbound_headers,
            &body_bytes,
            &request_metadata,
            &policy,
            &target,
        )
        .await;

        match attempt {
            AttemptResult::Retry(mut failure) => {
                failure.attempt_count = attempted_targets;
                context
                    .circuit_breakers
                    .record_failure(
                        route.agent,
                        target.id(),
                        circuit_config.clone(),
                        permit.used_half_open_permit,
                        &failure.summary,
                    )
                    .await;
                last_failure = Some(failure);
            }
            AttemptResult::Return(mut upstream) => {
                upstream.attempt_count = attempted_targets;
                completion.set_target(
                    target.id(),
                    target.name(),
                    &upstream.endpoint,
                    &upstream.outbound_model,
                    attempted_targets,
                );

                if upstream.error_summary.is_some() {
                    context
                        .circuit_breakers
                        .release_neutral(
                            route.agent,
                            target.id(),
                            circuit_config.clone(),
                            permit.used_half_open_permit,
                        )
                        .await;
                } else if upstream.is_streaming_body() {
                    upstream.attach_stream_completion(StreamCompletion {
                        completion,
                        circuit_breakers: context.circuit_breakers.clone(),
                        circuit_config: circuit_config.clone(),
                        agent: route.agent,
                        target_id: target.id().to_string(),
                        used_half_open_permit: permit.used_half_open_permit,
                    });
                    mark_active_target(&context, route.agent, target.id()).await;
                    return upstream.into_response(route.agent);
                } else {
                    context
                        .circuit_breakers
                        .record_success(
                            route.agent,
                            target.id(),
                            circuit_config.clone(),
                            permit.used_half_open_permit,
                        )
                        .await;
                    mark_active_target(&context, route.agent, target.id()).await;
                }

                let usage = upstream.capture_usage(route.agent);
                let status = upstream.status;
                let is_streaming = upstream.is_streaming;
                let error_summary = upstream.error_summary.clone();
                let response = upstream.into_response(route.agent);
                completion
                    .complete(usage, status.as_u16(), is_streaming, error_summary)
                    .await;
                return response;
            }
        }
    }

    let failure = last_failure.unwrap_or_else(|| AttemptFailure {
        status: StatusCode::SERVICE_UNAVAILABLE,
        upstream_status: None,
        headers: HeaderMap::new(),
        body: Bytes::new(),
        summary: if saw_circuit_candidate {
            "all configured upstream targets failed".to_string()
        } else {
            "all configured upstream targets are circuit-open or busy probing".to_string()
        },
        endpoint: route.forward_path.clone(),
        outbound_model: request_metadata.model.clone(),
        target_id: String::new(),
        target_name: String::new(),
        attempt_count: attempted_targets,
    });
    completion.set_target(
        &failure.target_id,
        &failure.target_name,
        &failure.endpoint,
        &failure.outbound_model,
        failure.attempt_count,
    );
    let status = failure.upstream_status.unwrap_or(failure.status);
    let summary = failure.summary.clone();
    completion
        .complete(
            TokenUsage::default(),
            status.as_u16(),
            request_metadata.streaming,
            Some(summary.clone()),
        )
        .await;
    if failure.upstream_status.is_some() {
        buffered_response(status, failure.headers, failure.body)
    } else {
        json_error(status, &summary)
    }
}

async fn mark_active_target(context: &ServerContext, agent: RouterAgent, target_id: &str) {
    let mut config = context.config.write().await;
    config.upstreams.agent_mut(agent).policy.active_target = target_id.to_string();
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectedRoute {
    agent: RouterAgent,
    forward_path: String,
}

impl SelectedRoute {
    fn bridges_responses_to_chat(&self, target: &UpstreamTarget) -> bool {
        if self.agent != RouterAgent::Codex || !target.enable_chat_completions_proxy() {
            return false;
        }
        matches!(
            self.forward_path.trim_end_matches('/'),
            "/responses" | "/v1/responses"
        )
    }

    fn semantic_protocol(&self, target: &UpstreamTarget) -> Option<SemanticProtocol> {
        if self.agent == RouterAgent::Claude && self.forward_path.starts_with("/v1/messages") {
            return Some(SemanticProtocol::Anthropic);
        }
        if self.agent != RouterAgent::Codex {
            return None;
        }
        if self.bridges_responses_to_chat(target) || self.forward_path.contains("/chat/completions")
        {
            Some(SemanticProtocol::ChatCompletions)
        } else if self.forward_path.contains("/responses") {
            Some(SemanticProtocol::Responses)
        } else {
            None
        }
    }
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
        || path.starts_with("/v1/models")
        || path == "/models"
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
    filtered.remove(ROUTER_TOKEN_HEADER);
    filtered
}

fn request_is_authorized(
    config: &RouterRuntimeConfig,
    agent: RouterAgent,
    headers: &HeaderMap,
) -> bool {
    let Ok(listen_addr) = super::validate_listen_address(&config.listen_address, config.port)
    else {
        return false;
    };
    if listen_addr.ip().is_loopback() {
        return true;
    }

    let provided = router_credentials(headers);
    if provided
        .iter()
        .any(|credential| constant_time_secret_eq(credential, &config.access_token))
    {
        return true;
    }

    config
        .upstreams
        .agent(agent)
        .candidates()
        .first()
        .map(|target| target.api_key())
        .filter(|api_key| !api_key.is_empty())
        .is_some_and(|api_key| {
            provided
                .iter()
                .any(|credential| constant_time_secret_eq(credential, api_key))
        })
}

fn router_credentials(headers: &HeaderMap) -> Vec<&str> {
    let mut credentials = Vec::with_capacity(3);
    for header in [ROUTER_TOKEN_HEADER, "x-api-key"] {
        if let Some(value) = headers.get(header).and_then(|value| value.to_str().ok()) {
            credentials.push(value.trim());
        }
    }
    if let Some(value) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().strip_prefix("Bearer "))
    {
        credentials.push(value.trim());
    }
    credentials
}

fn constant_time_secret_eq(provided: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    let provided = Sha256::digest(provided.as_bytes());
    let expected = Sha256::digest(expected.as_bytes());
    provided
        .iter()
        .zip(expected.iter())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn strip_router_credentials(headers: &mut HeaderMap, access_token: &str) {
    headers.remove(ROUTER_TOKEN_HEADER);
    for header in [AUTHORIZATION.as_str(), "x-api-key"] {
        let is_router_token = headers
            .get(header)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .map(|value| value.strip_prefix("Bearer ").unwrap_or(value).trim())
            .is_some_and(|value| constant_time_secret_eq(value, access_token));
        if is_router_token {
            headers.remove(header);
        }
    }
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

#[allow(clippy::too_many_arguments)]
async fn attempt_target(
    runtime_config: &RouterRuntimeConfig,
    route: &SelectedRoute,
    method: &Method,
    query: Option<&str>,
    original_headers: &HeaderMap,
    original_body: &[u8],
    request_metadata: &RequestMetadata,
    policy: &RouterAgentPolicy,
    target: &UpstreamTarget,
) -> AttemptResult {
    let prepared = transforms::prepare_request(original_body, target, policy);
    let request_model = prepared.request_model.clone();
    let outbound_model = prepared.outbound_model.clone();
    let mut one_m_context = prepared.one_m_context;
    let bridge = route.bridges_responses_to_chat(target);
    let endpoint = if bridge {
        "/v1/chat/completions".to_string()
    } else {
        route.forward_path.clone()
    };
    let upstream_url = match build_upstream_url(target, &endpoint, query) {
        Ok(url) => url,
        Err(message) => {
            return AttemptResult::Retry(AttemptFailure::transport(
                target,
                StatusCode::BAD_GATEWAY,
                message,
                endpoint,
                outbound_model,
            ));
        }
    };

    let mut current_json = prepared.json.clone();
    let mut current_body = match request_body_for_target(&prepared, bridge) {
        Ok(body) => body,
        Err(message) => {
            return AttemptResult::Return(AttemptResponse::local_error(
                StatusCode::BAD_REQUEST,
                message,
                endpoint,
                outbound_model,
            ));
        }
    };
    // Preflight optimization for Claude: inject thinking configuration and
    // prompt-cache breakpoints before the first upstream send. Both run only
    // when enabled by policy; the rectifier retry path below re-serializes the
    // body independently and does not re-run these passes.
    let mut interleaved_thinking = false;
    if route.agent == RouterAgent::Claude {
        if let Some(json) = current_json.as_mut() {
            if policy.thinking_optimizer_enabled {
                if let Some(outcome) = thinking_optimizer::optimize(json) {
                    one_m_context |= outcome.one_m_context;
                    interleaved_thinking |= outcome.interleaved_thinking_beta;
                    current_body = serde_json::to_vec(json).unwrap_or(current_body);
                }
            }
            if policy.cache_injection_enabled {
                cache_injector::inject(json);
                current_body = serde_json::to_vec(json).unwrap_or(current_body);
            }
        }
    }

    let mut rectifier_retried = false;

    loop {
        let headers = match outbound_headers(
            original_headers,
            route.agent,
            target,
            bridge,
            one_m_context,
            interleaved_thinking,
        ) {
            Ok(headers) => headers,
            Err(message) => {
                return AttemptResult::Retry(AttemptFailure::transport(
                    target,
                    StatusCode::BAD_GATEWAY,
                    message,
                    endpoint,
                    outbound_model,
                ));
            }
        };

        let request = runtime_config
            .client()
            .request(method.clone(), upstream_url.clone())
            .headers(headers)
            .body(current_body.clone());
        let stream_started_at = Instant::now();
        let mut raw = match send_upstream(
            request,
            method,
            request_metadata.streaming,
            policy.auto_failover_enabled,
            policy.streaming_first_byte_timeout,
            policy.non_streaming_timeout,
        )
        .await
        {
            Ok(response) => response,
            Err(failure) => {
                return AttemptResult::Retry(AttemptFailure::transport(
                    target,
                    failure.status,
                    failure.summary,
                    endpoint,
                    outbound_model,
                ));
            }
        };

        if raw.status.as_u16() >= 400 {
            let status = raw.status;
            let (headers, body) = raw.into_buffered();
            let summary = upstream_error_summary(status, &headers, &body);
            if !rectifier_retried && policy.rectifier_enabled && route.agent == RouterAgent::Claude
            {
                if let Some(json) = current_json.as_ref() {
                    if let Some((rectified, _kind)) =
                        transforms::rectify_request_for_error(json, &summary)
                    {
                        current_json = Some(rectified.clone());
                        current_body = serde_json::to_vec(&rectified).unwrap_or(current_body);
                        rectifier_retried = true;
                        continue;
                    }
                }
            }

            let response = AttemptResponse {
                status,
                headers,
                body: AttemptBody::Buffered(body),
                is_streaming: request_metadata.streaming,
                error_summary: Some(summary.clone()),
                endpoint: endpoint.clone(),
                outbound_model: outbound_model.clone(),
                attempt_count: 0,
            };
            if !should_retry_status(route.agent, target, status) {
                return AttemptResult::Return(response);
            }
            return AttemptResult::Retry(AttemptFailure {
                status,
                upstream_status: Some(status),
                headers: response.headers,
                body: match response.body {
                    AttemptBody::Buffered(body) => body,
                    AttemptBody::Streaming(_) => Bytes::new(),
                },
                summary,
                endpoint,
                outbound_model,
                target_id: target.id().to_string(),
                target_name: target.name().to_string(),
                attempt_count: 0,
            });
        }

        let semantic_protocol = policy
            .auto_failover_enabled
            .then(|| route.semantic_protocol(target))
            .flatten();
        if let Some(protocol) = semantic_protocol {
            raw = match validate_success_response(
                raw,
                protocol,
                stream_started_at,
                policy.streaming_first_byte_timeout,
            )
            .await
            {
                Ok(raw) => raw,
                Err(failure) => {
                    return AttemptResult::Retry(AttemptFailure::transport(
                        target,
                        failure.status,
                        failure.summary,
                        endpoint,
                        outbound_model,
                    ));
                }
            };
        }

        return match raw.body {
            RawAttemptBody::Buffered(body) => {
                let mut headers = raw.headers;
                let body = if bridge {
                    match serde_json::from_slice::<Value>(&body) {
                        Ok(payload) => {
                            headers.remove(CONTENT_ENCODING);
                            headers.remove(CONTENT_LENGTH);
                            headers
                                .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                            serde_json::to_vec(&chat_response_to_responses(
                                &payload,
                                if outbound_model.is_empty() {
                                    &request_model
                                } else {
                                    &outbound_model
                                },
                            ))
                            .map(Bytes::from)
                            .unwrap_or(body)
                        }
                        Err(_) => body,
                    }
                } else {
                    body
                };
                AttemptResult::Return(AttemptResponse {
                    status: raw.status,
                    headers,
                    body: AttemptBody::Buffered(body),
                    is_streaming: request_metadata.streaming,
                    error_summary: None,
                    endpoint,
                    outbound_model,
                    attempt_count: 0,
                })
            }
            RawAttemptBody::Streaming { first, rest } => {
                let mut headers = raw.headers;
                headers.remove(CONTENT_LENGTH);
                if bridge {
                    headers.remove(CONTENT_ENCODING);
                    headers.insert(
                        CONTENT_TYPE,
                        HeaderValue::from_static("text/event-stream; charset=utf-8"),
                    );
                }
                let idle_timeout = if policy.auto_failover_enabled {
                    policy.streaming_idle_timeout
                } else {
                    0
                };
                let semantic_protocol =
                    semantic_protocol.filter(|_| !response_body_is_encoded(&headers));
                AttemptResult::Return(AttemptResponse {
                    status: raw.status,
                    headers,
                    body: AttemptBody::Streaming(StreamingAttemptBody {
                        first,
                        rest,
                        bridge_model: bridge.then(|| {
                            if outbound_model.is_empty() {
                                request_model.clone()
                            } else {
                                outbound_model.clone()
                            }
                        }),
                        semantic_protocol,
                        idle_timeout,
                        stream_completion: None,
                    }),
                    is_streaming: true,
                    error_summary: None,
                    endpoint,
                    outbound_model,
                    attempt_count: 0,
                })
            }
        };
    }
}

fn request_body_for_target(prepared: &PreparedRequest, bridge: bool) -> Result<Vec<u8>, String> {
    if !bridge {
        return Ok(prepared.bytes.clone());
    }
    let json = prepared
        .json
        .as_ref()
        .ok_or_else(|| "Codex Responses-to-Chat bridge requires a JSON request body".to_string())?;
    serde_json::to_vec(&responses_to_chat(json))
        .map_err(|error| format!("failed to convert Responses request: {error}"))
}

fn outbound_headers(
    original: &HeaderMap,
    agent: RouterAgent,
    target: &UpstreamTarget,
    bridge: bool,
    one_m_context: bool,
    interleaved_thinking: bool,
) -> Result<HeaderMap, String> {
    let mut headers = original.clone();
    headers.remove(CONTENT_LENGTH);
    if bridge {
        headers.remove(ACCEPT_ENCODING);
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }

    if !target.api_key().is_empty() {
        headers.remove(AUTHORIZATION);
        headers.remove("x-api-key");
        let key = HeaderValue::from_str(target.api_key())
            .map_err(|_| "target API key contains invalid header characters".to_string())?;
        match agent {
            RouterAgent::Claude => {
                headers.insert("x-api-key", key);
                let bearer = HeaderValue::from_str(&format!("Bearer {}", target.api_key()))
                    .map_err(|_| "target API key contains invalid header characters".to_string())?;
                headers.insert(AUTHORIZATION, bearer);
            }
            RouterAgent::Codex => {
                let bearer = HeaderValue::from_str(&format!("Bearer {}", target.api_key()))
                    .map_err(|_| "target API key contains invalid header characters".to_string())?;
                headers.insert(AUTHORIZATION, bearer);
            }
        }
    }

    if agent == RouterAgent::Claude && (one_m_context || interleaved_thinking) {
        let mut values = headers
            .get("anthropic-beta")
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if one_m_context
            && !values
                .iter()
                .any(|value| value.eq_ignore_ascii_case(ANTHROPIC_ONE_M_BETA))
        {
            values.push(ANTHROPIC_ONE_M_BETA.to_string());
        }
        if interleaved_thinking
            && !values.iter().any(|value| {
                value.eq_ignore_ascii_case(thinking_optimizer::INTERLEAVED_THINKING_BETA)
            })
        {
            values.push(thinking_optimizer::INTERLEAVED_THINKING_BETA.to_string());
        }
        let value = HeaderValue::from_str(&values.join(","))
            .map_err(|_| "failed to construct anthropic-beta header".to_string())?;
        headers.insert("anthropic-beta", value);
    }

    Ok(headers)
}

struct TransportFailure {
    status: StatusCode,
    summary: String,
}

struct RawAttemptResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: RawAttemptBody,
}

impl RawAttemptResponse {
    fn into_buffered(self) -> (HeaderMap, Bytes) {
        match self.body {
            RawAttemptBody::Buffered(body) => (self.headers, body),
            RawAttemptBody::Streaming { .. } => (self.headers, Bytes::new()),
        }
    }
}

enum RawAttemptBody {
    Buffered(Bytes),
    Streaming {
        first: Option<Bytes>,
        rest: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticProtocol {
    Anthropic,
    Responses,
    ChatCompletions,
}

enum StreamStartInspection {
    Pending,
    Safe,
    Failed(String),
}

async fn send_upstream(
    request: reqwest::RequestBuilder,
    method: &Method,
    streaming: bool,
    timeouts_enabled: bool,
    first_byte_timeout: u64,
    non_streaming_timeout: u64,
) -> Result<RawAttemptResponse, TransportFailure> {
    if streaming {
        let operation = async {
            let response = request.send().await.map_err(transport_from_reqwest)?;
            let status = response.status();
            let headers = filter_response_headers(response.headers().clone());
            if status.as_u16() >= 400
                || *method == Method::HEAD
                || matches!(status, StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED)
            {
                let body = read_response_body_limited(response).await?;
                return Ok(RawAttemptResponse {
                    status,
                    headers,
                    body: RawAttemptBody::Buffered(body),
                });
            }
            let mut stream = response.bytes_stream().boxed();
            let first = stream
                .next()
                .await
                .transpose()
                .map_err(transport_from_reqwest)?;
            Ok(RawAttemptResponse {
                status,
                headers,
                body: RawAttemptBody::Streaming {
                    first,
                    rest: stream,
                },
            })
        };
        if timeouts_enabled && first_byte_timeout > 0 {
            tokio::time::timeout(Duration::from_secs(first_byte_timeout), operation)
                .await
                .map_err(|_| TransportFailure {
                    status: StatusCode::GATEWAY_TIMEOUT,
                    summary: format!(
                        "upstream did not produce its first response chunk within {first_byte_timeout}s"
                    ),
                })?
        } else {
            operation.await
        }
    } else {
        let operation = async {
            let response = request.send().await.map_err(transport_from_reqwest)?;
            let status = response.status();
            let headers = filter_response_headers(response.headers().clone());
            let body = if *method == Method::HEAD
                || matches!(status, StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED)
            {
                Bytes::new()
            } else {
                read_response_body_limited(response).await?
            };
            Ok(RawAttemptResponse {
                status,
                headers,
                body: RawAttemptBody::Buffered(body),
            })
        };
        if timeouts_enabled && non_streaming_timeout > 0 {
            tokio::time::timeout(Duration::from_secs(non_streaming_timeout), operation)
                .await
                .map_err(|_| TransportFailure {
                    status: StatusCode::GATEWAY_TIMEOUT,
                    summary: format!(
                        "upstream non-streaming request exceeded {non_streaming_timeout}s"
                    ),
                })?
        } else {
            operation.await
        }
    }
}

async fn validate_success_response(
    response: RawAttemptResponse,
    protocol: SemanticProtocol,
    stream_started_at: Instant,
    semantic_timeout_seconds: u64,
) -> Result<RawAttemptResponse, TransportFailure> {
    let RawAttemptResponse {
        status,
        headers,
        body,
    } = response;
    let body = match body {
        RawAttemptBody::Buffered(body) => {
            let decoded = decompress_upstream_body(&headers, &body);
            if let Some(summary) = semantic_error_from_bytes(protocol, &decoded) {
                return Err(TransportFailure {
                    status: StatusCode::BAD_GATEWAY,
                    summary,
                });
            }
            RawAttemptBody::Buffered(body)
        }
        RawAttemptBody::Streaming { first, rest } => {
            if response_body_is_encoded(&headers) {
                RawAttemptBody::Streaming { first, rest }
            } else {
                prime_streaming_body(
                    first,
                    rest,
                    protocol,
                    stream_started_at,
                    semantic_timeout_seconds,
                )
                .await?
            }
        }
    };
    Ok(RawAttemptResponse {
        status,
        headers,
        body,
    })
}

fn response_body_is_encoded(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .is_some_and(|value| !value.is_empty() && !value.eq_ignore_ascii_case("identity"))
}

async fn prime_streaming_body(
    first: Option<Bytes>,
    mut rest: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    protocol: SemanticProtocol,
    stream_started_at: Instant,
    semantic_timeout_seconds: u64,
) -> Result<RawAttemptBody, TransportFailure> {
    let mut buffered = Vec::new();
    if let Some(first) = first {
        buffered.extend_from_slice(&first);
    }

    loop {
        match inspect_stream_start(protocol, &buffered, false) {
            StreamStartInspection::Safe => {
                return Ok(RawAttemptBody::Streaming {
                    first: Some(Bytes::from(buffered)),
                    rest,
                });
            }
            StreamStartInspection::Failed(summary) => {
                return Err(TransportFailure {
                    status: StatusCode::BAD_GATEWAY,
                    summary,
                });
            }
            StreamStartInspection::Pending => {}
        }

        if buffered.len() >= MAX_STREAM_PRIME_BYTES {
            return Err(TransportFailure {
                status: StatusCode::BAD_GATEWAY,
                summary: format!(
                    "upstream produced no semantic output within the {} KiB stream preflight limit",
                    MAX_STREAM_PRIME_BYTES / 1024
                ),
            });
        }

        let next = if semantic_timeout_seconds == 0 {
            rest.next().await
        } else {
            let remaining = Duration::from_secs(semantic_timeout_seconds)
                .saturating_sub(stream_started_at.elapsed());
            if remaining.is_zero() {
                return Err(TransportFailure {
                    status: StatusCode::GATEWAY_TIMEOUT,
                    summary: format!(
                        "upstream produced no semantic output within {semantic_timeout_seconds}s"
                    ),
                });
            }
            tokio::time::timeout(remaining, rest.next())
                .await
                .map_err(|_| TransportFailure {
                    status: StatusCode::GATEWAY_TIMEOUT,
                    summary: format!(
                        "upstream produced no semantic output within {semantic_timeout_seconds}s"
                    ),
                })?
        };

        match next {
            Some(Ok(chunk)) => buffered.extend_from_slice(&chunk),
            Some(Err(error)) => return Err(transport_from_reqwest(error)),
            None => {
                return match inspect_stream_start(protocol, &buffered, true) {
                    StreamStartInspection::Safe => Ok(RawAttemptBody::Streaming {
                        first: (!buffered.is_empty()).then(|| Bytes::from(buffered)),
                        rest,
                    }),
                    StreamStartInspection::Failed(summary) => Err(TransportFailure {
                        status: StatusCode::BAD_GATEWAY,
                        summary,
                    }),
                    StreamStartInspection::Pending => Err(TransportFailure {
                        status: StatusCode::BAD_GATEWAY,
                        summary: "upstream stream ended before producing semantic output"
                            .to_string(),
                    }),
                };
            }
        }
    }
}

fn inspect_stream_start(
    protocol: SemanticProtocol,
    buffered: &[u8],
    end_of_stream: bool,
) -> StreamStartInspection {
    let trimmed = buffered
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|start| &buffered[start..])
        .unwrap_or_default();
    if matches!(trimmed.first(), Some(b'{') | Some(b'[')) {
        if let Ok(value) = serde_json::from_slice::<Value>(trimmed) {
            return semantic_error_from_value(protocol, &value)
                .map(StreamStartInspection::Failed)
                .unwrap_or(StreamStartInspection::Safe);
        }
    }

    let normalized = String::from_utf8_lossy(buffered).replace("\r\n", "\n");
    let blocks = normalized.split("\n\n").collect::<Vec<_>>();
    let complete_blocks = if end_of_stream {
        blocks.len()
    } else {
        blocks.len().saturating_sub(1)
    };
    for block in blocks.into_iter().take(complete_blocks) {
        match inspect_sse_block(protocol, block) {
            StreamStartInspection::Pending => {}
            result => return result,
        }
    }
    StreamStartInspection::Pending
}

fn inspect_sse_block(protocol: SemanticProtocol, block: &str) -> StreamStartInspection {
    let mut named_event = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = line.strip_prefix("event:") {
            named_event = Some(event.trim());
        } else if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start());
        }
    }
    if data_lines.is_empty() {
        return StreamStartInspection::Pending;
    }
    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        return StreamStartInspection::Safe;
    }
    let Ok(value) = serde_json::from_str::<Value>(&data) else {
        return StreamStartInspection::Pending;
    };
    if let Some(summary) = semantic_error_from_value(protocol, &value) {
        return StreamStartInspection::Failed(summary);
    }
    let event = named_event
        .filter(|event| !event.is_empty())
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or_default();
    if event == "error" || event == "response.failed" {
        return StreamStartInspection::Failed(format!(
            "{} upstream emitted {event} before output",
            semantic_protocol_name(protocol)
        ));
    }

    match protocol {
        SemanticProtocol::Anthropic => match event {
            "message_start" | "content_block_start" | "ping" | "" => StreamStartInspection::Pending,
            _ => StreamStartInspection::Safe,
        },
        SemanticProtocol::Responses => match event {
            "response.created"
            | "response.in_progress"
            | "response.queued"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.reasoning_summary_part.added"
            | "" => StreamStartInspection::Pending,
            _ => StreamStartInspection::Safe,
        },
        SemanticProtocol::ChatCompletions => {
            if chat_chunk_has_output(&value) {
                StreamStartInspection::Safe
            } else {
                StreamStartInspection::Pending
            }
        }
    }
}

fn chat_chunk_has_output(value: &Value) -> bool {
    if value.get("usage").is_some_and(|usage| !usage.is_null()) {
        return true;
    }
    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                if choice
                    .get("finish_reason")
                    .is_some_and(|reason| !reason.is_null())
                {
                    return true;
                }
                let Some(delta) = choice.get("delta") else {
                    return false;
                };
                delta
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| !content.is_empty())
                    || [
                        "tool_calls",
                        "function_call",
                        "reasoning",
                        "reasoning_content",
                    ]
                    .into_iter()
                    .any(|key| delta.get(key).is_some_and(|part| !part.is_null()))
            })
        })
}

fn semantic_error_from_bytes(protocol: SemanticProtocol, body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    semantic_error_from_value(protocol, &value)
}

fn semantic_error_from_value(protocol: SemanticProtocol, value: &Value) -> Option<String> {
    let payload = if protocol == SemanticProtocol::Responses {
        value.get("response").unwrap_or(value)
    } else {
        value
    };
    let status = payload.get("status").and_then(Value::as_str);
    let error = payload.get("error").filter(|error| !error.is_null());
    let explicit_error = payload.get("type").and_then(Value::as_str) == Some("error");
    let failed = match protocol {
        SemanticProtocol::Responses => {
            matches!(status, Some("failed" | "cancelled")) || error.is_some()
        }
        SemanticProtocol::Anthropic | SemanticProtocol::ChatCompletions => {
            explicit_error || error.is_some()
        }
    };
    if !failed {
        return None;
    }

    let detail = error.unwrap_or(payload);
    let kind = detail
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| detail.get("code").and_then(Value::as_str))
        .or(status)
        .unwrap_or("upstream_error");
    let message = detail
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| detail.as_str())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or("upstream reported a semantic failure");
    Some(format!(
        "{} upstream {kind}: {message}",
        semantic_protocol_name(protocol)
    ))
}

fn semantic_protocol_name(protocol: SemanticProtocol) -> &'static str {
    match protocol {
        SemanticProtocol::Anthropic => "Anthropic",
        SemanticProtocol::Responses => "Responses",
        SemanticProtocol::ChatCompletions => "Chat Completions",
    }
}

struct SemanticStreamObserver {
    protocol: SemanticProtocol,
    pending: Vec<u8>,
}

impl SemanticStreamObserver {
    fn new(protocol: SemanticProtocol) -> Self {
        Self {
            protocol,
            pending: Vec::new(),
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Option<String> {
        self.pending.extend_from_slice(chunk);
        while let Some((index, delimiter_len)) = find_sse_delimiter(&self.pending) {
            let block = self
                .pending
                .drain(..index + delimiter_len)
                .collect::<Vec<_>>();
            if let StreamStartInspection::Failed(summary) =
                inspect_sse_block(self.protocol, &String::from_utf8_lossy(&block[..index]))
            {
                return Some(summary);
            }
        }
        if self.pending.len() > MAX_STREAM_PRIME_BYTES {
            self.pending.clear();
        }
        None
    }

    fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let block = std::mem::take(&mut self.pending);
        match inspect_sse_block(self.protocol, &String::from_utf8_lossy(&block)) {
            StreamStartInspection::Failed(summary) => Some(summary),
            StreamStartInspection::Pending | StreamStartInspection::Safe => None,
        }
    }
}

fn find_sse_delimiter(bytes: &[u8]) -> Option<(usize, usize)> {
    for index in 0..bytes.len() {
        if bytes.get(index..index + 2) == Some(b"\n\n") {
            return Some((index, 2));
        }
        if bytes.get(index..index + 4) == Some(b"\r\n\r\n") {
            return Some((index, 4));
        }
    }
    None
}

async fn read_response_body_limited(
    response: reqwest::Response,
) -> Result<Bytes, TransportFailure> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(transport_from_reqwest)?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BODY_BYTES {
            return Err(TransportFailure {
                status: StatusCode::BAD_GATEWAY,
                summary: "upstream response exceeded the local router limit".to_string(),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

fn transport_from_reqwest(error: reqwest::Error) -> TransportFailure {
    if error.is_timeout() {
        TransportFailure {
            status: StatusCode::GATEWAY_TIMEOUT,
            summary: "upstream request timed out".to_string(),
        }
    } else if error.is_connect() {
        TransportFailure {
            status: StatusCode::BAD_GATEWAY,
            summary: "failed to connect to upstream".to_string(),
        }
    } else {
        TransportFailure {
            status: StatusCode::BAD_GATEWAY,
            summary: "upstream request or response stream failed".to_string(),
        }
    }
}

fn should_retry_status(agent: RouterAgent, target: &UpstreamTarget, status: StatusCode) -> bool {
    if agent == RouterAgent::Codex
        && matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        && target.api_key().is_empty()
        && target
            .base_url()
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("chatgpt.com"))
        && target
            .base_url()
            .path()
            .trim_end_matches('/')
            .ends_with("/backend-api/codex")
    {
        return false;
    }
    !NON_RETRYABLE_STATUS_CODES.contains(&status.as_u16())
}

/// Decompress an upstream error response body for inspection. Mirrors the
/// encoding handling used for usage capture so compressed error responses are
/// still legible. Falls back to the raw body when the encoding is unknown or the
/// decoder fails, so a malformed body never masks the real upstream status.
fn decompress_upstream_body(headers: &HeaderMap, body: &[u8]) -> Vec<u8> {
    let Some(encoding) = headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.eq_ignore_ascii_case("identity"))
    else {
        return body.to_vec();
    };
    let Some(decoded) = usage::decode_for_inspection(body, Some(encoding)) else {
        return body.to_vec();
    };
    decoded
}

fn upstream_error_summary(status: StatusCode, headers: &HeaderMap, body: &[u8]) -> String {
    let decoded = decompress_upstream_body(headers, body);
    let parsed = serde_json::from_slice::<Value>(&decoded).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let text = String::from_utf8_lossy(body);
            let text = text.trim();
            (!text.is_empty()).then(|| text.chars().take(512).collect())
        })
        .unwrap_or_else(|| "upstream rejected the request".to_string());
    format!("upstream returned HTTP {}: {message}", status.as_u16())
}

enum AttemptResult {
    Return(AttemptResponse),
    Retry(AttemptFailure),
}

struct AttemptFailure {
    status: StatusCode,
    upstream_status: Option<StatusCode>,
    headers: HeaderMap,
    body: Bytes,
    summary: String,
    endpoint: String,
    outbound_model: String,
    target_id: String,
    target_name: String,
    attempt_count: usize,
}

impl AttemptFailure {
    fn transport(
        target: &UpstreamTarget,
        status: StatusCode,
        summary: impl Into<String>,
        endpoint: String,
        outbound_model: String,
    ) -> Self {
        Self {
            status,
            upstream_status: None,
            headers: HeaderMap::new(),
            body: Bytes::new(),
            summary: summary.into(),
            endpoint,
            outbound_model,
            target_id: target.id().to_string(),
            target_name: target.name().to_string(),
            attempt_count: 0,
        }
    }
}

struct AttemptResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: AttemptBody,
    is_streaming: bool,
    error_summary: Option<String>,
    endpoint: String,
    outbound_model: String,
    attempt_count: usize,
}

impl AttemptResponse {
    fn local_error(
        status: StatusCode,
        message: String,
        endpoint: String,
        outbound_model: String,
    ) -> Self {
        let body = Bytes::from(
            serde_json::to_vec(&json!({
                "error": {"type": "local_router_error", "message": message}
            }))
            .unwrap_or_default(),
        );
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Self {
            status,
            headers,
            body: AttemptBody::Buffered(body),
            is_streaming: false,
            error_summary: Some(message),
            endpoint,
            outbound_model,
            attempt_count: 0,
        }
    }

    fn is_streaming_body(&self) -> bool {
        matches!(self.body, AttemptBody::Streaming(_))
    }

    fn attach_stream_completion(&mut self, completion: StreamCompletion) {
        if let AttemptBody::Streaming(stream) = &mut self.body {
            stream.stream_completion = Some(completion);
        }
    }

    fn capture_usage(&self, agent: RouterAgent) -> TokenUsage {
        let AttemptBody::Buffered(body) = &self.body else {
            return TokenUsage::default();
        };
        let content_encoding = self
            .headers
            .get(CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok());
        let mut capture = UsageCapture::new(agent, self.is_streaming, content_encoding);
        capture.push(body);
        capture.finish()
    }

    fn into_response(self, agent: RouterAgent) -> Response {
        match self.body {
            AttemptBody::Buffered(body) => buffered_response(self.status, self.headers, body),
            AttemptBody::Streaming(streaming) => {
                streaming_response(self.status, self.headers, streaming, agent)
            }
        }
    }
}

enum AttemptBody {
    Buffered(Bytes),
    Streaming(StreamingAttemptBody),
}

struct StreamingAttemptBody {
    first: Option<Bytes>,
    rest: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    bridge_model: Option<String>,
    semantic_protocol: Option<SemanticProtocol>,
    idle_timeout: u64,
    stream_completion: Option<StreamCompletion>,
}

struct StreamCompletion {
    completion: RequestCompletion,
    circuit_breakers: Arc<CircuitBreakerRegistry>,
    circuit_config: CircuitBreakerConfig,
    agent: RouterAgent,
    target_id: String,
    used_half_open_permit: bool,
}

fn streaming_response(
    status: StatusCode,
    mut headers: HeaderMap,
    streaming: StreamingAttemptBody,
    agent: RouterAgent,
) -> Response {
    headers.remove(CONTENT_LENGTH);
    let mut state = ProxyBodyState {
        upstream: streaming.rest,
        pending: VecDeque::new(),
        transformer: streaming.bridge_model.map(ChatSseTransformer::new),
        semantic_observer: streaming.semantic_protocol.map(SemanticStreamObserver::new),
        semantic_failure: None,
        capture: Some(UsageCapture::new(agent, true, None)),
        stream_completion: streaming.stream_completion,
        status_code: status.as_u16(),
        idle_timeout: streaming.idle_timeout,
        upstream_finished: false,
        finalized: false,
    };
    if let Some(first) = streaming.first {
        state.accept_upstream_chunk(&first);
    }
    let stream = stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        loop {
            if state.upstream_finished && !state.finalized {
                if let Some(summary) = state
                    .semantic_observer
                    .as_mut()
                    .and_then(SemanticStreamObserver::finish)
                {
                    state.finalize_failure(&summary).await;
                } else {
                    state.finalize_success().await;
                }
            }
            if let Some(chunk) = state.pending.pop_front() {
                return Some((Ok::<Bytes, io::Error>(chunk), Some(state)));
            }
            if let Some(summary) = state.semantic_failure.take() {
                state.finalize_failure(&summary).await;
                return None;
            }
            if state.upstream_finished {
                return None;
            }

            match state.next_upstream_chunk().await {
                Ok(Some(chunk)) => state.accept_upstream_chunk(&chunk),
                Ok(None) => {
                    if let Some(transformer) = state.transformer.as_mut() {
                        for chunk in transformer.finish() {
                            state.enqueue_output(chunk);
                        }
                    }
                    state.upstream_finished = true;
                }
                Err(summary) => {
                    state.finalize_failure(&summary).await;
                    return Some((Err(io::Error::other(summary)), None));
                }
            }
        }
    });

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

struct ProxyBodyState {
    upstream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    pending: VecDeque<Bytes>,
    transformer: Option<ChatSseTransformer>,
    semantic_observer: Option<SemanticStreamObserver>,
    semantic_failure: Option<String>,
    capture: Option<UsageCapture>,
    stream_completion: Option<StreamCompletion>,
    status_code: u16,
    idle_timeout: u64,
    upstream_finished: bool,
    finalized: bool,
}

impl ProxyBodyState {
    fn accept_upstream_chunk(&mut self, chunk: &[u8]) {
        if self.semantic_failure.is_none() {
            self.semantic_failure = self
                .semantic_observer
                .as_mut()
                .and_then(|observer| observer.push(chunk));
        }
        if let Some(transformer) = self.transformer.as_mut() {
            for output in transformer.push(chunk) {
                self.enqueue_output(output);
            }
        } else {
            self.enqueue_output(Bytes::copy_from_slice(chunk));
        }
    }

    fn enqueue_output(&mut self, chunk: Bytes) {
        if let Some(capture) = self.capture.as_mut() {
            capture.push(&chunk);
        }
        self.pending.push_back(chunk);
    }

    async fn next_upstream_chunk(&mut self) -> Result<Option<Bytes>, String> {
        let next = self.upstream.next();
        let result = if self.idle_timeout == 0 {
            next.await
        } else {
            tokio::time::timeout(Duration::from_secs(self.idle_timeout), next)
                .await
                .map_err(|_| {
                    format!(
                        "upstream response stream was silent for {}s",
                        self.idle_timeout
                    )
                })?
        };
        result
            .transpose()
            .map_err(|error| transport_from_reqwest(error).summary)
    }

    async fn finalize_success(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        let usage = self
            .capture
            .take()
            .map(UsageCapture::finish)
            .unwrap_or_default();
        if let Some(stream_completion) = self.stream_completion.take() {
            stream_completion
                .circuit_breakers
                .record_success(
                    stream_completion.agent,
                    &stream_completion.target_id,
                    stream_completion.circuit_config,
                    stream_completion.used_half_open_permit,
                )
                .await;
            stream_completion
                .completion
                .complete(usage, self.status_code, true, None)
                .await;
        }
    }

    async fn finalize_failure(&mut self, summary: &str) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        let usage = self
            .capture
            .take()
            .map(UsageCapture::finish)
            .unwrap_or_default();
        if let Some(stream_completion) = self.stream_completion.take() {
            stream_completion
                .circuit_breakers
                .record_failure(
                    stream_completion.agent,
                    &stream_completion.target_id,
                    stream_completion.circuit_config,
                    stream_completion.used_half_open_permit,
                    summary,
                )
                .await;
            stream_completion
                .completion
                .complete(usage, self.status_code, true, Some(summary.to_string()))
                .await;
        }
    }
}

impl Drop for ProxyBodyState {
    fn drop(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        let usage = self
            .capture
            .take()
            .map(UsageCapture::finish)
            .unwrap_or_default();
        let Some(stream_completion) = self.stream_completion.take() else {
            return;
        };
        let summary = "client disconnected before the upstream response completed".to_string();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                stream_completion
                    .circuit_breakers
                    .release_neutral(
                        stream_completion.agent,
                        &stream_completion.target_id,
                        stream_completion.circuit_config,
                        stream_completion.used_half_open_permit,
                    )
                    .await;
                stream_completion
                    .completion
                    .complete(usage, 0, true, Some(summary))
                    .await;
            });
        } else {
            stream_completion
                .completion
                .complete_without_runtime(&summary);
        }
    }
}

struct CompletionContext {
    request_id: String,
    session_id: Option<String>,
    agent: RouterAgent,
    request_model: String,
    outbound_model: Option<String>,
    target_id: Option<String>,
    target_name: Option<String>,
    endpoint: String,
    attempt_count: usize,
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
                session_id: None,
                agent,
                request_model: String::new(),
                outbound_model: None,
                target_id: None,
                target_name: None,
                endpoint: String::new(),
                attempt_count: 0,
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

    fn set_session_id(&mut self, session_id: Option<String>) {
        if let Some(context) = self.context.as_mut() {
            context.session_id = session_id;
        }
    }

    fn set_streaming(&mut self, streaming: bool) {
        if let Some(context) = self.context.as_mut() {
            context.is_streaming = streaming;
        }
    }

    fn set_endpoint(&mut self, endpoint: String) {
        if let Some(context) = self.context.as_mut() {
            context.endpoint = endpoint;
        }
    }

    fn set_target(
        &mut self,
        target_id: &str,
        target_name: &str,
        endpoint: &str,
        outbound_model: &str,
        attempt_count: usize,
    ) {
        if let Some(context) = self.context.as_mut() {
            context.target_id = (!target_id.is_empty()).then(|| target_id.to_string());
            context.target_name = (!target_name.is_empty()).then(|| target_name.to_string());
            context.endpoint = endpoint.to_string();
            context.outbound_model =
                (!outbound_model.trim().is_empty()).then(|| outbound_model.to_string());
            context.attempt_count = attempt_count;
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

    fn complete_without_runtime(mut self, error_summary: &str) {
        let Some(context) = self.context.take() else {
            return;
        };
        context.metrics.finish_request(false);
        context
            .metrics
            .set_error(Some(context.agent), error_summary);
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
        .or_else(|| context.outbound_model.clone())
        .or_else(|| {
            (!context.request_model.trim().is_empty()).then_some(context.request_model.clone())
        })
        .unwrap_or_else(|| "unknown".to_string());
    let record = RouterRequestRecord {
        request_id: context.request_id,
        session_id: context.session_id,
        response_id: usage.response_id,
        agent: context.agent,
        target_id: context.target_id,
        target_name: context.target_name,
        endpoint: context.endpoint,
        attempt_count: context.attempt_count.min(u32::MAX as usize) as u32,
        model,
        outbound_model: context.outbound_model,
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

fn buffered_response(status: StatusCode, headers: HeaderMap, body: Bytes) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
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

fn unauthorized_response() -> Response {
    let mut response = json_error(
        StatusCode::UNAUTHORIZED,
        "a valid local router access token is required",
    );
    response.headers_mut().insert(
        "www-authenticate",
        HeaderValue::from_static("Bearer realm=\"Aeroric Local Router\""),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_router::{
        LocalRouterState, RouterAgentRuntime, RouterUpstreams, UpstreamTarget, ROUTE_AGENT_HEADER,
    };
    use axum::http::HeaderValue;
    use std::convert::Infallible;
    use std::fs;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::{Path, PathBuf};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    #[test]
    fn retry_policy_protects_official_codex_auth_but_allows_other_accounts() {
        let official = UpstreamTarget::with_details(
            "codex",
            "Codex",
            "https://chatgpt.com/backend-api/codex",
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        assert!(!should_retry_status(
            RouterAgent::Codex,
            &official,
            StatusCode::UNAUTHORIZED
        ));
        assert!(!should_retry_status(
            RouterAgent::Codex,
            &official,
            StatusCode::FORBIDDEN
        ));

        let third_party = UpstreamTarget::with_details(
            "custom",
            "Custom",
            "https://gateway.example.test/v1",
            "provider-key",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        assert!(should_retry_status(
            RouterAgent::Codex,
            &third_party,
            StatusCode::UNAUTHORIZED
        ));
        assert!(!should_retry_status(
            RouterAgent::Codex,
            &third_party,
            StatusCode::BAD_REQUEST
        ));
    }

    #[test]
    fn semantic_failure_detection_does_not_reject_incomplete_responses() {
        let failed = json!({
            "status": "failed",
            "error": {"type": "server_error", "message": "busy"},
            "output": []
        });
        assert!(
            semantic_error_from_value(SemanticProtocol::Responses, &failed)
                .is_some_and(|summary| summary.contains("busy"))
        );

        let incomplete = json!({
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "output": []
        });
        assert_eq!(
            semantic_error_from_value(SemanticProtocol::Responses, &incomplete),
            None
        );
    }

    #[test]
    fn stream_priming_waits_through_lifecycle_events_and_catches_failure() {
        let lifecycle = b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, lifecycle, false),
            StreamStartInspection::Pending
        ));

        let structural = b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"message\",\"content\":[]}}\n\nevent: response.content_part.added\ndata: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, structural, false),
            StreamStartInspection::Pending
        ));

        let failed = b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"overloaded\"}}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, failed, false),
            StreamStartInspection::Failed(summary) if summary.contains("overloaded")
        ));

        let output = b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Responses, output, false),
            StreamStartInspection::Safe
        ));
    }

    #[test]
    fn anthropic_stream_priming_waits_for_content_after_block_start() {
        let structural = b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Anthropic, structural, false),
            StreamStartInspection::Pending
        ));

        let output = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::Anthropic, output, false),
            StreamStartInspection::Safe
        ));
    }

    #[test]
    fn chat_stream_role_only_chunk_is_not_committed_as_output() {
        let role = b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::ChatCompletions, role, false),
            StreamStartInspection::Pending
        ));
        let content =
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n";
        assert!(matches!(
            inspect_stream_start(SemanticProtocol::ChatCompletions, content, false),
            StreamStartInspection::Safe
        ));
    }

    #[test]
    fn semantic_stream_observer_detects_failure_after_output() {
        let mut observer = SemanticStreamObserver::new(SemanticProtocol::Responses);
        assert_eq!(
            observer.push(
                b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n"
            ),
            None
        );
        assert_eq!(
            observer.push(
                b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"late failure\"}}}\n"
            ),
            None
        );
        assert!(observer
            .push(b"\n")
            .is_some_and(|summary| summary.contains("late failure")));
    }

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
        headers.insert(
            ROUTER_TOKEN_HEADER,
            HeaderValue::from_static("router-secret"),
        );
        headers.insert(HOST, HeaderValue::from_static("127.0.0.1"));
        let filtered = filter_request_headers(headers);
        assert_eq!(filtered["authorization"], "Bearer secret");
        assert_eq!(filtered["x-api-key"], "secret");
        assert!(!filtered.contains_key(CONNECTION));
        assert!(!filtered.contains_key("x-remove"));
        assert!(!filtered.contains_key(ROUTE_AGENT_HEADER));
        assert!(!filtered.contains_key(ROUTER_TOKEN_HEADER));
        assert!(!filtered.contains_key(HOST));
    }

    #[test]
    fn non_loopback_requests_require_router_or_active_upstream_credentials() {
        let target = UpstreamTarget::with_details(
            "codex",
            "Codex",
            "https://api.example.test/v1",
            "upstream-secret",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let config = RouterRuntimeConfig::new(
            "0.0.0.0",
            43123,
            false,
            runtime_with_target(
                RouterAgent::Codex,
                target,
                RouterAgentPolicy {
                    active_target: "codex".to_string(),
                    ..RouterAgentPolicy::default()
                },
            ),
        )
        .with_access_token("aeroric-0123456789abcdef0123456789abcdef");

        assert!(!request_is_authorized(
            &config,
            RouterAgent::Codex,
            &HeaderMap::new()
        ));

        let mut router_token = HeaderMap::new();
        router_token.insert(
            ROUTER_TOKEN_HEADER,
            HeaderValue::from_static("aeroric-0123456789abcdef0123456789abcdef"),
        );
        assert!(request_is_authorized(
            &config,
            RouterAgent::Codex,
            &router_token
        ));

        let mut upstream_token = HeaderMap::new();
        upstream_token.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer upstream-secret"),
        );
        assert!(request_is_authorized(
            &config,
            RouterAgent::Codex,
            &upstream_token
        ));

        let mut wrong_token = HeaderMap::new();
        wrong_token.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-secret"),
        );
        assert!(!request_is_authorized(
            &config,
            RouterAgent::Codex,
            &wrong_token
        ));
    }

    #[test]
    fn loopback_requests_remain_compatible_without_credentials() {
        let config =
            RouterRuntimeConfig::new("127.0.0.1", 43123, false, RouterUpstreams::default());
        assert!(request_is_authorized(
            &config,
            RouterAgent::Claude,
            &HeaderMap::new()
        ));
    }

    #[test]
    fn router_access_credentials_are_not_forwarded_upstream() {
        let access_token = "aeroric-0123456789abcdef0123456789abcdef";
        let mut headers = HeaderMap::new();
        headers.insert(
            ROUTER_TOKEN_HEADER,
            HeaderValue::from_static("must-not-forward"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer aeroric-0123456789abcdef0123456789abcdef"),
        );
        strip_router_credentials(&mut headers, access_token);
        assert!(!headers.contains_key(ROUTER_TOKEN_HEADER));
        assert!(!headers.contains_key(AUTHORIZATION));
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

    fn remove_database(path: &Path) {
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

    fn runtime_with_target(
        agent: RouterAgent,
        target: UpstreamTarget,
        policy: RouterAgentPolicy,
    ) -> RouterUpstreams {
        let runtime = RouterAgentRuntime {
            targets: vec![target],
            policy,
        };
        match agent {
            RouterAgent::Claude => RouterUpstreams {
                claude: runtime,
                codex: RouterAgentRuntime::default(),
            },
            RouterAgent::Codex => RouterUpstreams {
                claude: RouterAgentRuntime::default(),
                codex: runtime,
            },
        }
    }

    fn failover_upstreams(
        agent: RouterAgent,
        targets: Vec<UpstreamTarget>,
        target_ids: &[&str],
    ) -> RouterUpstreams {
        let runtime = RouterAgentRuntime {
            targets,
            policy: RouterAgentPolicy {
                auto_failover_enabled: true,
                max_retries: target_ids.len().saturating_sub(1) as u8,
                active_target: target_ids.first().copied().unwrap_or_default().to_string(),
                failover_queue: target_ids.iter().map(|id| (*id).to_string()).collect(),
                ..RouterAgentPolicy::default()
            },
        };
        match agent {
            RouterAgent::Claude => RouterUpstreams {
                claude: runtime,
                codex: RouterAgentRuntime::default(),
            },
            RouterAgent::Codex => RouterUpstreams {
                claude: RouterAgentRuntime::default(),
                codex: runtime,
            },
        }
    }

    #[tokio::test]
    async fn forwards_json_and_records_target_usage_without_sensitive_request_data() {
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
        let target = UpstreamTarget::with_details(
            "codex",
            "Codex",
            format!("http://{upstream_address}/api/v1"),
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                local_port,
                true,
                runtime_with_target(
                    RouterAgent::Codex,
                    target,
                    RouterAgentPolicy {
                        active_target: "codex".to_string(),
                        ..RouterAgentPolicy::default()
                    },
                ),
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
        assert_eq!(recent[0].target_id.as_deref(), Some("codex"));
        assert_eq!(recent[0].endpoint, "/v1/responses");
        assert_eq!(recent[0].attempt_count, 1);
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
    async fn failover_uses_queue_order_and_records_attempt_count() {
        let failing = Router::new().fallback(any(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":{"message":"down"}})),
            )
        }));
        let healthy = Router::new().fallback(any(|| async {
            Json(json!({"id":"resp_ok","model":"gpt-test","output":[],"usage":{}}))
        }));
        let (failing_address, failing_task) = start_mock_upstream(failing).await;
        let (healthy_address, healthy_task) = start_mock_upstream(healthy).await;
        let first = UpstreamTarget::with_details(
            "first",
            "First",
            format!("http://{failing_address}/v1"),
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let second = UpstreamTarget::with_details(
            "second",
            "Second",
            format!("http://{healthy_address}/v1"),
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let policy = RouterAgentPolicy {
            auto_failover_enabled: true,
            max_retries: 1,
            failover_queue: vec!["first".to_string(), "second".to_string()],
            active_target: "first".to_string(),
            ..RouterAgentPolicy::default()
        };
        let upstreams = RouterUpstreams {
            claude: RouterAgentRuntime::default(),
            codex: RouterAgentRuntime {
                targets: vec![first, second],
                policy,
            },
        };
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                upstreams,
            ))
            .await
            .unwrap();
        let response = reqwest::Client::new()
            .post(format!("{}/codex/v1/responses", info.base_url))
            .json(&json!({"model":"gpt-test","input":"hello"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let records = router.recent_requests(5).await.unwrap();
        assert_eq!(records[0].target_id.as_deref(), Some("second"));
        assert_eq!(records[0].attempt_count, 2);

        router.stop().await.unwrap();
        failing_task.abort();
        healthy_task.abort();
        let _ = failing_task.await;
        let _ = healthy_task.await;
        remove_database(&database_path);
    }

    #[tokio::test]
    async fn buffered_2xx_semantic_failure_fails_over() {
        let failing = Router::new().fallback(any(|| async {
            Json(json!({
                "status": "failed",
                "error": {"type": "server_error", "message": "primary overloaded"},
                "output": []
            }))
        }));
        let healthy = Router::new().fallback(any(|| async {
            Json(json!({"id":"resp_fallback","status":"completed","output":[],"usage":{}}))
        }));
        let (failing_address, failing_task) = start_mock_upstream(failing).await;
        let (healthy_address, healthy_task) = start_mock_upstream(healthy).await;
        let targets = vec![
            UpstreamTarget::with_details(
                "first",
                "First",
                format!("http://{failing_address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
            UpstreamTarget::with_details(
                "second",
                "Second",
                format!("http://{healthy_address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
        ];
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                failover_upstreams(RouterAgent::Codex, targets, &["first", "second"]),
            ))
            .await
            .unwrap();

        let response = reqwest::Client::new()
            .post(format!("{}/codex/v1/responses", info.base_url))
            .json(&json!({"model":"gpt-test","input":"hello"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<Value>().await.unwrap()["id"],
            "resp_fallback"
        );
        let records = router.recent_requests(5).await.unwrap();
        assert_eq!(records[0].target_id.as_deref(), Some("second"));
        assert_eq!(records[0].attempt_count, 2);

        router.stop().await.unwrap();
        failing_task.abort();
        healthy_task.abort();
        let _ = failing_task.await;
        let _ = healthy_task.await;
        remove_database(&database_path);
    }

    #[tokio::test]
    async fn response_failed_before_stream_output_fails_over() {
        let failing = Router::new().fallback(any(|| async {
            let chunks = vec![Ok::<Bytes, Infallible>(Bytes::from_static(
                b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"stream overloaded\"}}}\n\n",
            ))];
            let mut response = Response::new(Body::from_stream(stream::iter(chunks)));
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            response
        }));
        let healthy = Router::new().fallback(any(|| async {
            let chunks = vec![Ok::<Bytes, Infallible>(Bytes::from_static(
                b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"fallback output\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
            ))];
            let mut response = Response::new(Body::from_stream(stream::iter(chunks)));
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            response
        }));
        let (failing_address, failing_task) = start_mock_upstream(failing).await;
        let (healthy_address, healthy_task) = start_mock_upstream(healthy).await;
        let targets = vec![
            UpstreamTarget::with_details(
                "first",
                "First",
                format!("http://{failing_address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
            UpstreamTarget::with_details(
                "second",
                "Second",
                format!("http://{healthy_address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
        ];
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                failover_upstreams(RouterAgent::Codex, targets, &["first", "second"]),
            ))
            .await
            .unwrap();

        let response = reqwest::Client::new()
            .post(format!("{}/codex/v1/responses", info.base_url))
            .json(&json!({"model":"gpt-test","input":"hello","stream":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.unwrap();
        assert!(body.contains("fallback output"));
        assert!(!body.contains("stream overloaded"));
        let records = router.recent_requests(5).await.unwrap();
        assert_eq!(records[0].target_id.as_deref(), Some("second"));
        assert_eq!(records[0].attempt_count, 2);

        router.stop().await.unwrap();
        failing_task.abort();
        healthy_task.abort();
        let _ = failing_task.await;
        let _ = healthy_task.await;
        remove_database(&database_path);
    }

    #[tokio::test]
    async fn response_failed_after_stream_output_marks_the_target_unhealthy() {
        let upstream = Router::new().fallback(any(|| async {
            let chunks = vec![
                Ok::<Bytes, Infallible>(Bytes::from_static(
                    b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
                )),
                Ok::<Bytes, Infallible>(Bytes::from_static(
                    b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"late failure\"}}}\n\n",
                )),
            ];
            let mut response = Response::new(Body::from_stream(stream::iter(chunks)));
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
            response
        }));
        let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;
        let target = UpstreamTarget::with_details(
            "only",
            "Only",
            format!("http://{upstream_address}/v1"),
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let config = RouterRuntimeConfig::new(
            "127.0.0.1",
            unused_port(),
            true,
            failover_upstreams(RouterAgent::Codex, vec![target], &["only"]),
        );
        let info = router.start(config.clone()).await.unwrap();

        let response = reqwest::Client::new()
            .post(format!("{}/codex/v1/responses", info.base_url))
            .json(&json!({"model":"gpt-test","input":"hello","stream":true}))
            .send()
            .await
            .unwrap();
        let body = response.text().await.unwrap();
        assert!(body.contains("partial"));
        assert!(body.contains("late failure"));
        let records = router.recent_requests(5).await.unwrap();
        assert!(!records[0].success);
        assert!(records[0]
            .error_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("late failure")));
        let targets = router.target_statuses(&config).await;
        assert_eq!(targets[0].circuit.consecutive_failures, 1);

        router.stop().await.unwrap();
        upstream_task.abort();
        let _ = upstream_task.await;
        remove_database(&database_path);
    }

    #[tokio::test]
    async fn non_retryable_http_error_does_not_fail_over() {
        let second_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let failing = Router::new().fallback(any(|| async {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":{"message":"bad input"}})),
            )
        }));
        let hits = second_hits.clone();
        let healthy = Router::new().fallback(any(move || {
            let hits = hits.clone();
            async move {
                hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Json(json!({"id":"unexpected"}))
            }
        }));
        let (failing_address, failing_task) = start_mock_upstream(failing).await;
        let (healthy_address, healthy_task) = start_mock_upstream(healthy).await;
        let targets = vec![
            UpstreamTarget::with_details(
                "first",
                "First",
                format!("http://{failing_address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
            UpstreamTarget::with_details(
                "second",
                "Second",
                format!("http://{healthy_address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
        ];
        let upstreams = RouterUpstreams {
            claude: RouterAgentRuntime::default(),
            codex: RouterAgentRuntime {
                targets,
                policy: RouterAgentPolicy {
                    auto_failover_enabled: true,
                    max_retries: 3,
                    failover_queue: vec!["first".to_string(), "second".to_string()],
                    ..RouterAgentPolicy::default()
                },
            },
        };
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                upstreams,
            ))
            .await
            .unwrap();
        let response = reqwest::Client::new()
            .post(format!("{}/codex/v1/responses", info.base_url))
            .json(&json!({"model":"gpt-test","input":"bad"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(second_hits.load(std::sync::atomic::Ordering::SeqCst), 0);

        router.stop().await.unwrap();
        failing_task.abort();
        healthy_task.abort();
        let _ = failing_task.await;
        let _ = healthy_task.await;
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
        let target = UpstreamTarget::with_details(
            "claude",
            "Claude",
            format!("http://{upstream_address}"),
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                runtime_with_target(
                    RouterAgent::Claude,
                    target,
                    RouterAgentPolicy {
                        active_target: "claude".to_string(),
                        ..RouterAgentPolicy::default()
                    },
                ),
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

    #[tokio::test]
    async fn bridges_codex_responses_to_chat_completions() {
        let upstream = Router::new().fallback(any(|request: Request| async move {
            assert_eq!(request.uri().path(), "/v1/chat/completions");
            let body = to_bytes(request.into_body(), MAX_REQUEST_BODY_BYTES)
                .await
                .unwrap();
            let json: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json["messages"][0]["role"], "user");
            Json(json!({
                "id":"chat_1",
                "model":"gpt-test",
                "choices":[{"message":{"role":"assistant","content":"Hello"}}],
                "usage":{"prompt_tokens":4,"completion_tokens":2}
            }))
        }));
        let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;
        let target = UpstreamTarget::with_details(
            "chat",
            "Chat",
            format!("http://{upstream_address}/v1"),
            "",
            vec!["gpt-test".to_string()],
            false,
            true,
        )
        .unwrap();
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone()).unwrap();
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                runtime_with_target(
                    RouterAgent::Codex,
                    target,
                    RouterAgentPolicy {
                        active_target: "chat".to_string(),
                        ..RouterAgentPolicy::default()
                    },
                ),
            ))
            .await
            .unwrap();
        let response = reqwest::Client::new()
            .post(format!("{}/codex/v1/responses", info.base_url))
            .json(&json!({
                "model":"gpt-test",
                "input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"Hello"}]}],
                "stream":false
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: Value = response.json().await.unwrap();
        assert_eq!(payload["object"], "response");
        assert_eq!(payload["output"][0]["content"][0]["text"], "Hello");

        router.stop().await.unwrap();
        upstream_task.abort();
        let _ = upstream_task.await;
        remove_database(&database_path);
    }

    fn build_error_summary(encoding: &str, body: &[u8]) -> String {
        let mut headers = HeaderMap::new();
        if !encoding.eq_ignore_ascii_case("identity") {
            headers.insert(
                axum::http::HeaderName::from_bytes(b"content-encoding").unwrap(),
                HeaderValue::from_str(encoding).unwrap(),
            );
        }
        upstream_error_summary(StatusCode::BAD_REQUEST, &headers, body)
    }

    #[test]
    fn upstream_error_summary_decompresses_gzip() {
        let json = br#"{"error":{"message":"compressed gzip failure"}}"#;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        encoder.write_all(json).unwrap();
        let body = encoder.finish().unwrap();
        let summary = build_error_summary("gzip", &body);
        assert!(summary.contains("HTTP 400"));
        assert!(summary.contains("compressed gzip failure"));
    }

    #[test]
    fn upstream_error_summary_decompresses_brotli() {
        use std::io::Write;
        let json = br#"{"error":{"message":"compressed brotli failure"}}"#;
        let mut writer = brotli::CompressorWriter::new(Vec::new(), 4096, 6, 22);
        writer.write_all(json).unwrap();
        writer.flush().unwrap();
        let body = writer.into_inner();
        let summary = build_error_summary("br", &body);
        assert!(summary.contains("compressed brotli failure"));
    }

    #[test]
    fn upstream_error_summary_decompresses_zstd() {
        let json = br#"{"error":{"message":"compressed zstd failure"}}"#;
        let body = zstd::encode_all(json.as_slice(), 3).unwrap();
        let summary = build_error_summary("zstd", &body);
        assert!(summary.contains("compressed zstd failure"));
    }
}
