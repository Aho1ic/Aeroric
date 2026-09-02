use super::cache_injector;
use super::chat_bridge::{chat_response_to_responses, responses_to_chat, ChatSseTransformer};
use super::circuit_breaker::{CircuitBreakerConfig, CircuitBreakerRegistry, CircuitPermit};
use super::inline_tool_calls;
use super::session;
use super::thinking_optimizer;
use super::transforms::{self, PreparedRequest};
use super::usage::{self, TokenUsage, UsageCapture, UsageStore};
use super::{
    RouterAgent, RouterAgentPolicy, RouterRequestRecord, RouterRuntimeConfig, RuntimeMetrics,
    UpstreamTarget, HEALTH_PATH, ROUTER_TOKEN_HEADER, ROUTE_AGENT_HEADER,
};
use crate::sse::find_sse_delimiter;
use axum::body::{to_bytes, Body, Bytes};

// 三段实现搬进了子模块。它们都以 `use super::*;` 开头,直接拿这里的
// import 和私有类型;父模块反过来用 `use` 把它们的项拉回本作用域,
// 于是原有调用点一处都不用改。
mod guard;
mod routing;
mod semantic;

use axum::extract::{Request, State};
use axum::http::header::{
    ACCEPT_ENCODING, AUTHORIZATION, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE,
    HOST, ORIGIN,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use futures_util::stream::{self, BoxStream};
use futures_util::StreamExt;
use guard::{
    filter_request_headers, filter_response_headers, request_is_authorized, request_is_cross_site,
    strip_router_credentials,
};
use routing::{build_upstream_url, select_route, SelectedRoute};
use semantic::{inspect_stream_start, semantic_error_from_bytes, SemanticStreamObserver};
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
const SEC_FETCH_SITE_HEADER: &str = "sec-fetch-site";

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

async fn health_check(headers: HeaderMap) -> Response {
    if request_is_cross_site(&headers) {
        return cross_site_response();
    }
    Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
    .into_response()
}

async fn proxy_request(State(context): State<ServerContext>, request: Request) -> Response {
    let started = Instant::now();
    let started_at = usage::unix_millis();
    let (mut parts, body) = request.into_parts();
    if request_is_cross_site(&parts.headers) {
        return cross_site_response();
    }
    let route = match select_route(&parts.uri, &parts.headers) {
        Ok(route) => route,
        Err(message) => return json_error(StatusCode::NOT_FOUND, message),
    };

    let runtime_config = context.config.read().await.clone();
    let agent_runtime = runtime_config.upstreams.agent(route.agent).clone();
    if !request_is_authorized(&runtime_config, &parts.headers) {
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

    let candidates = agent_runtime.candidates_for(route.target_id.as_deref());
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
    // 熔断器只在"还有别的目标可以接住流量"时才有意义。以下两种情况必须直通：
    //  - 未开启自动故障转移：候选永远只有一个 active_target
    //  - 请求指定了目标（/targets/<id>/…）：这是 Agent 配置的固定上游，不是路由偏好
    // 否则一次瞬时上游抖动就会把唯一目标熔断 circuit_timeout_seconds 秒，
    // 期间所有请求直接 503 "circuit-open or busy probing"，表现为该配置整体不可用。
    let single_pinned_target = route.target_id.is_some() || candidates.len() <= 1;
    let bypass_circuit_breaker = !policy.auto_failover_enabled || single_pinned_target;
    let mut attempted_targets = 0usize;
    let mut last_failure: Option<AttemptFailure> = None;
    let mut saw_circuit_candidate = false;
    let mut skipped_open_target: Option<UpstreamTarget> = None;

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
            if skipped_open_target.is_none() {
                skipped_open_target = Some(target);
            }
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
                    if route.target_id.is_none() {
                        mark_active_target(&context, route.agent, target.id()).await;
                    }
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
                    if route.target_id.is_none() {
                        mark_active_target(&context, route.agent, target.id()).await;
                    }
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

    // 所有候选都被熔断挡住时，宁可赌一次真实请求也不要凭空 503：真实请求最坏是拿到
    // 上游错误（并记为失败），而凭空 503 会让 agent 在整个熔断窗口内完全无法工作。
    if !saw_circuit_candidate {
        if let Some(target) = skipped_open_target {
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
                            false,
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
                    if upstream.error_summary.is_none() {
                        if upstream.is_streaming_body() {
                            upstream.attach_stream_completion(StreamCompletion {
                                completion,
                                circuit_breakers: context.circuit_breakers.clone(),
                                circuit_config: circuit_config.clone(),
                                agent: route.agent,
                                target_id: target.id().to_string(),
                                used_half_open_permit: false,
                            });
                            if route.target_id.is_none() {
                                mark_active_target(&context, route.agent, target.id()).await;
                            }
                            return upstream.into_response(route.agent);
                        }
                        context
                            .circuit_breakers
                            .record_success(route.agent, target.id(), circuit_config.clone(), false)
                            .await;
                        if route.target_id.is_none() {
                            mark_active_target(&context, route.agent, target.id()).await;
                        }
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
                // 非流式 Responses：部分中转会把工具调用写进 assistant 文本，
                // Codex 会原样打印那段标记且工具不执行，这里还原成原生 function_call。
                let body = if route.agent == RouterAgent::Codex
                    && route.forward_path.contains("/responses")
                    && !response_body_is_encoded(&headers)
                {
                    repair_inline_tool_calls_in_body(&body).unwrap_or(body)
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
                let body_is_encoded = response_body_is_encoded(&headers);
                let semantic_protocol = semantic_protocol.filter(|_| !body_is_encoded);
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
                        // 桥接过的流由 ChatSseTransformer 直接产出原生工具调用；
                        // 只有直连 Responses 的上游会把工具调用写进文本里。
                        repair_inline_tool_calls: !bridge
                            && route.agent == RouterAgent::Codex
                            && route.forward_path.contains("/responses")
                            && !body_is_encoded,
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
    /// 是否需要把文本形态的工具调用还原成原生 `function_call`(仅 Codex Responses 流)。
    repair_inline_tool_calls: bool,
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
        inline_tool_calls: streaming
            .repair_inline_tool_calls
            .then(inline_tool_calls::InlineToolCallSseFilter::new),
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
                    if let Some(filter) = state.inline_tool_calls.as_mut() {
                        for chunk in filter.finish() {
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
    inline_tool_calls: Option<inline_tool_calls::InlineToolCallSseFilter>,
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
        } else if let Some(filter) = self.inline_tool_calls.as_mut() {
            for output in filter.push(chunk) {
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
        // 这道 `finalized` 守卫在当前代码里是**第二道**:`finalize_success` /
        // `finalize_failure` 都无条件 `self.stream_completion.take()`,所以正常收尾之后
        // 下面那句 `let Some(..) = take() else { return }` 已经能拦住重复收尾。
        // 变异测试实测:单独去掉这一句,6 条压测用例全绿。
        //
        // **刻意保留。** 重复收尾的后果是同一个请求被记两次账(失败率虚高)并对熔断器
        // 重复投票;而这个标志同时被 `streaming_response` 的 unfold 循环读
        // (`state.upstream_finished && !state.finalized`),那处是承重的。
        // 让"已收尾"这件事在**每一条**出口自己成立,比依赖"另一处恰好把 Option 取空了"
        // 要稳 —— 谁再跑变异测试看到它存活,是这个原因,不是缺测试。
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

/// 客户端主动断开导致的收尾原因。用户按 Esc 打断、关闭任务、切走配置都会走到这里，
/// 属于正常操作而非路由故障，因此不能污染"最近错误"横幅和失败计数——否则用户每次
/// 打断都会看到一条红色报错。请求行仍会落库，方便在请求历史里排查。
const CLIENT_ABORT_SUMMARIES: [&str; 2] = [
    "client disconnected before the upstream response completed",
    "local router request was cancelled",
];

fn is_client_abort_summary(summary: &str) -> bool {
    CLIENT_ABORT_SUMMARIES.contains(&summary)
}

async fn finalize_request(
    context: CompletionContext,
    usage: TokenUsage,
    status_code: u16,
    error_summary: Option<String>,
) {
    let client_aborted = error_summary
        .as_deref()
        .is_some_and(is_client_abort_summary);
    let success = error_summary.is_none() && (200..400).contains(&status_code);
    if !client_aborted {
        context.metrics.finish_request(success);
    } else {
        context.metrics.finish_client_abort();
    }
    let error_summary = error_summary.as_deref().map(usage::sanitize_summary);
    if let Some(message) = error_summary.as_deref() {
        if !client_aborted {
            context.metrics.set_error(Some(context.agent), message);
        }
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

/// 把非流式 Responses body 里"文本形态的工具调用"改写成原生 `function_call`。
/// 没有需要改写的内容(或 body 不是 JSON)时返回 None，调用方保持原 body。
fn repair_inline_tool_calls_in_body(body: &Bytes) -> Option<Bytes> {
    if !inline_tool_calls::contains_sentinel(&String::from_utf8_lossy(body)) {
        return None;
    }
    let mut payload = serde_json::from_slice::<Value>(body).ok()?;
    if !inline_tool_calls::rewrite_response_payload(&mut payload) {
        return None;
    }
    serde_json::to_vec(&payload).ok().map(Bytes::from)
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

fn cross_site_response() -> Response {
    json_error(
        StatusCode::FORBIDDEN,
        "cross-site browser requests are not allowed",
    )
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
    fn non_loopback_requests_require_the_dedicated_router_token() {
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

        assert!(!request_is_authorized(&config, &HeaderMap::new()));

        let mut router_token = HeaderMap::new();
        router_token.insert(
            ROUTER_TOKEN_HEADER,
            HeaderValue::from_static("aeroric-0123456789abcdef0123456789abcdef"),
        );
        assert!(request_is_authorized(&config, &router_token));

        let mut upstream_token = HeaderMap::new();
        upstream_token.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer upstream-secret"),
        );
        assert!(!request_is_authorized(&config, &upstream_token));

        let mut wrong_token = HeaderMap::new();
        wrong_token.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-secret"),
        );
        assert!(!request_is_authorized(&config, &wrong_token));
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
        let router = LocalRouterState::with_database_path(database_path.clone());
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
        let router = LocalRouterState::with_database_path(database_path.clone());
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
        let router = LocalRouterState::with_database_path(database_path.clone());
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
        let router = LocalRouterState::with_database_path(database_path.clone());
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
        let router = LocalRouterState::with_database_path(database_path.clone());
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
        let router = LocalRouterState::with_database_path(database_path.clone());
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

    /// 单目标配置绝不能被熔断器锁死：没有第二个上游可以接住流量时，熔断只会让
    /// 该配置在整个窗口内彻底不可用（用户可见的表现是每个请求秒回 503
    /// "circuit-open or busy probing"）。
    #[tokio::test]
    async fn a_single_target_keeps_serving_after_repeated_upstream_errors() {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = hits.clone();
        // 前 4 次返回真实 503（正好达到默认 failure_threshold），第 5 次恢复。
        let upstream = Router::new().fallback(any(move || {
            let counter = counter.clone();
            async move {
                let seen = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if seen < 4 {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"Service temporarily unavailable"}})),
                    )
                        .into_response();
                }
                Json(json!({"id":"recovered"})).into_response()
            }
        }));
        let (address, upstream_task) = start_mock_upstream(upstream).await;
        let target = UpstreamTarget::with_details(
            "only",
            "Only",
            format!("http://{address}/v1"),
            "",
            Vec::new(),
            false,
            false,
        )
        .unwrap();
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone());
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                failover_upstreams(RouterAgent::Codex, vec![target], &["only"]),
            ))
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let url = format!("{}/codex/v1/responses", info.base_url);
        for _ in 0..4 {
            let response = client
                .post(&url)
                .json(&json!({"model":"gpt-test","input":"hello"}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }

        // 熔断窗口内的第 5 个请求仍必须真正打到上游，而不是被本地凭空拒掉。
        let response = client
            .post(&url)
            .json(&json!({"model":"gpt-test","input":"hello"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().await.unwrap().contains("recovered"));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 5);

        router.stop().await.unwrap();
        upstream_task.abort();
        let _ = upstream_task.await;
        remove_database(&database_path);
    }

    /// 目标限定路由（/targets/<id>/…）代表 Agent 自己的固定上游，同样不能被熔断
    /// 锁死——它没有故障转移队列可退。
    #[tokio::test]
    async fn a_pinned_target_route_is_not_gated_by_the_circuit_breaker() {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = hits.clone();
        let upstream = Router::new().fallback(any(move || {
            let counter = counter.clone();
            async move {
                let seen = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if seen < 4 {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"message":"busy"}})),
                    )
                        .into_response();
                }
                Json(json!({"id":"pinned-ok"})).into_response()
            }
        }));
        let (address, upstream_task) = start_mock_upstream(upstream).await;
        let targets = vec![
            UpstreamTarget::with_details(
                "primary",
                "Primary",
                format!("http://{address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
            UpstreamTarget::with_details(
                "secondary",
                "Secondary",
                format!("http://{address}/v1"),
                "",
                Vec::new(),
                false,
                false,
            )
            .unwrap(),
        ];
        let database_path = temp_database_path();
        let router = LocalRouterState::with_database_path(database_path.clone());
        let info = router
            .start(RouterRuntimeConfig::new(
                "127.0.0.1",
                unused_port(),
                true,
                failover_upstreams(RouterAgent::Codex, targets, &["primary", "secondary"]),
            ))
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let url = format!("{}/codex/targets/primary/v1/responses", info.base_url);
        for _ in 0..4 {
            let response = client
                .post(&url)
                .json(&json!({"model":"gpt-test","input":"hello"}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
        let response = client
            .post(&url)
            .json(&json!({"model":"gpt-test","input":"hello"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.text().await.unwrap().contains("pinned-ok"));

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
        let router = LocalRouterState::with_database_path(database_path.clone());
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
        let router = LocalRouterState::with_database_path(database_path.clone());
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

    /// 并发压力测试:**真起 loopback 服务、真发 HTTP 请求**,断言的是账目守恒。
    ///
    /// 上面那批用例都是单请求走通一条路径。这里补的是"多请求同时在飞"时的性质:
    /// `RequestCompletion` 是个 RAII 记账守卫(`new()` 里 `begin_request`,
    /// `complete()` / `Drop` 收尾),`ProxyBodyState` 又叠了第二层(流式响应的
    /// `Drop` 里补 `release_neutral`)。这两层守卫**每条退出路径都必须恰好收尾一次** ——
    /// 漏一次,健康面板的在途数就永久飘高;多一次,失败率被虚增。
    /// 单请求测不出来,因为收工时 `active == 0` 与"压根没记账"是同一个值
    /// (踩过一次,见 mod.rs 的 `metrics_stress_tests`)。
    ///
    /// 嵌在 `tests` 里面而不是并列,是为了直接吃现成的 `runtime_with_target` /
    /// `failover_upstreams` / `start_mock_upstream` / `temp_database_path`,
    /// 不必把它们的可见性放宽 —— 那会改到既有测试代码。
    mod stress_tests {
        use super::*;
        use crate::local_router::RouterStatus;
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

        /// 轮询等账目稳定下来。
        ///
        /// **不能用固定 sleep。** 两条 `Drop` 路径都是 `runtime.spawn(...)` 出去的,
        /// 收尾发生在客户端拿到响应**之后**的某个不确定时刻;写死 sleep 要么在负载高时
        /// flake,要么为了保险睡很久拖慢整轮。这里改成"在途归零就停",超时才报错。
        async fn wait_until_settled(
            router: &LocalRouterState,
            expected_total: u64,
        ) -> RouterStatus {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let status = router.status().await;
                if status.active_requests == 0 && status.total_requests >= expected_total {
                    return status;
                }
                if Instant::now() >= deadline {
                    panic!(
                        "账目 10 秒内没有稳定:active={} total={}(期望 total≥{expected_total})",
                        status.active_requests, status.total_requests
                    );
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }

        /// 轮询等落库行数到位。
        ///
        /// **计数器稳定 ≠ 请求行已落库。** `finalize_request` 先 `finish_client_abort()`
        /// 再 `usage_store.insert(...).await`(server.rs:1717 与 1770),两者之间隔着一次
        /// await。所以在 `Drop` 那条 spawn 出去的路径上,`active_requests` 归零时
        /// insert 可能还在飞 —— 实测 6 个断开请求只读到 3 行。凡是要断言落库内容的
        /// 用例都得单独等这一步,别拿 `wait_until_settled` 的结论当落库完成。
        async fn wait_for_rows(
            router: &LocalRouterState,
            expected: usize,
        ) -> Vec<RouterRequestRecord> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let rows = router.recent_requests(expected * 2).await.unwrap();
                if rows.len() >= expected {
                    return rows;
                }
                if Instant::now() >= deadline {
                    panic!("10 秒内只落了 {} 行,期望 {expected} 行", rows.len());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
        async fn concurrent_real_requests_keep_the_metrics_ledger_balanced() {
            const REQUESTS: usize = 64;
            let served = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&served);
            let upstream = Router::new().fallback(any(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, AtomicOrdering::SeqCst);
                    Json(json!({
                        "id": "resp_concurrent",
                        "model": "gpt-5.6",
                        "usage": {"input_tokens": 3, "output_tokens": 1},
                        "output": []
                    }))
                }
            }));
            let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;

            let database_path = temp_database_path();
            let router = Arc::new(LocalRouterState::with_database_path(database_path.clone()));
            let target = UpstreamTarget::with_details(
                "codex",
                "Codex",
                format!("http://{upstream_address}/v1"),
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

            let client = reqwest::Client::new();
            let url = format!("{}/codex/v1/responses", info.base_url);
            let mut handles = Vec::with_capacity(REQUESTS);
            for _ in 0..REQUESTS {
                let client = client.clone();
                let url = url.clone();
                handles.push(tokio::spawn(async move {
                    client
                        .post(url)
                        .json(&json!({"model": "gpt-5.6", "input": "hi"}))
                        .send()
                        .await
                        .map(|response| response.status())
                }));
            }
            for handle in handles {
                let status = handle.await.unwrap().unwrap();
                assert_eq!(status, StatusCode::OK);
            }

            assert_eq!(
                served.load(AtomicOrdering::SeqCst),
                REQUESTS,
                "每个请求都必须真打到上游一次"
            );
            let status = wait_until_settled(&router, REQUESTS as u64).await;
            assert_eq!(status.active_requests, 0, "在途数必须归零");
            assert_eq!(status.total_requests, REQUESTS as u64);
            assert_eq!(
                status.successful_requests, REQUESTS as u64,
                "64 个 200 必须全部计成功"
            );
            assert_eq!(status.failed_requests, 0);
            assert_eq!(status.last_error, None, "全成功的一轮不该留下任何错误横幅");

            // 落库那侧也要守恒:request_id 是每个 RequestCompletion 各自的 uuid,
            // 并发下少一行就说明有一次收尾被丢了。
            let recent = router.recent_requests(REQUESTS * 2).await.unwrap();
            assert_eq!(recent.len(), REQUESTS, "落库行数必须等于请求数");
            let mut ids: Vec<&str> = recent.iter().map(|row| row.request_id.as_str()).collect();
            ids.sort_unstable();
            let issued = ids.len();
            ids.dedup();
            assert_eq!(ids.len(), issued, "request_id 不能重复");
            assert!(recent.iter().all(|row| row.success));

            router.stop().await.unwrap();
            upstream_task.abort();
            let _ = upstream_task.await;
            remove_database(&database_path);
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
        async fn in_flight_requests_are_visible_and_do_not_block_the_health_probe() {
            // 拿栅栏把 8 个请求**停在上游处理器里**(已记账、未收尾),此刻:
            //   1. `active_requests` 必须等于 8 —— 只在收工后断言 0 是空断言,
            //      因为 0 也是"从没记账过"的值;
            //   2. `/health` 必须**立刻**答 200 —— 代理路径若在某处握着全局锁,
            //      健康探针会跟着卡住,而前端正是靠它判断服务活着。
            const REQUESTS: usize = 8;
            // +1 是主测试自己那一份。
            let gate = Arc::new(tokio::sync::Barrier::new(REQUESTS + 1));
            let upstream_gate = Arc::clone(&gate);
            let upstream = Router::new().fallback(any(move || {
                let gate = Arc::clone(&upstream_gate);
                async move {
                    gate.wait().await;
                    Json(json!({"id": "resp_gated", "model": "gpt-5.6", "output": []}))
                }
            }));
            let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;

            let database_path = temp_database_path();
            let router = Arc::new(LocalRouterState::with_database_path(database_path.clone()));
            let target = UpstreamTarget::with_details(
                "codex",
                "Codex",
                format!("http://{upstream_address}/v1"),
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

            let client = reqwest::Client::new();
            let url = format!("{}/codex/v1/responses", info.base_url);
            let mut handles = Vec::with_capacity(REQUESTS);
            for _ in 0..REQUESTS {
                let client = client.clone();
                let url = url.clone();
                handles.push(tokio::spawn(async move {
                    client
                        .post(url)
                        .json(&json!({"model": "gpt-5.6", "input": "hi"}))
                        .send()
                        .await
                        .map(|response| response.status())
                }));
            }

            // 等到 8 个请求都记了账。轮询而不是 sleep,理由同 `wait_until_settled`。
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let status = router.status().await;
                if status.active_requests == REQUESTS as u64 {
                    assert_eq!(
                        status.successful_requests + status.failed_requests,
                        0,
                        "还没收尾就不该有成功或失败计数"
                    );
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "10 秒内在途数没到 {REQUESTS},实际 {}",
                    status.active_requests
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }

            // 8 个请求全部卡在上游期间,健康探针必须秒回。
            let health = tokio::time::timeout(
                Duration::from_secs(2),
                client.get(format!("{}{HEALTH_PATH}", info.base_url)).send(),
            )
            .await
            .expect("在途请求把 /health 卡住了 —— 代理路径上有全局锁")
            .unwrap();
            assert_eq!(health.status(), StatusCode::OK);
            assert_eq!(health.json::<Value>().await.unwrap()["status"], "healthy");

            gate.wait().await;
            for handle in handles {
                assert_eq!(handle.await.unwrap().unwrap(), StatusCode::OK);
            }
            let status = wait_until_settled(&router, REQUESTS as u64).await;
            assert_eq!(status.successful_requests, REQUESTS as u64);
            assert_eq!(status.failed_requests, 0);

            router.stop().await.unwrap();
            upstream_task.abort();
            let _ = upstream_task.await;
            remove_database(&database_path);
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
        async fn concurrent_failovers_do_not_lose_or_double_count_attempts() {
            // 第一顺位**永远** 503(可重试),第二顺位健康。32 路并发下:
            // 全部 200、成功计数正好 32、失败计数 0(转移成功不算请求失败)。
            let dead_hits = Arc::new(AtomicUsize::new(0));
            let dead_counter = Arc::clone(&dead_hits);
            let dead = Router::new().fallback(any(move || {
                let counter = Arc::clone(&dead_counter);
                async move {
                    counter.fetch_add(1, AtomicOrdering::SeqCst);
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error": {"message": "down"}})),
                    )
                }
            }));
            let (dead_address, dead_task) = start_mock_upstream(dead).await;

            let alive_hits = Arc::new(AtomicUsize::new(0));
            let alive_counter = Arc::clone(&alive_hits);
            let alive = Router::new().fallback(any(move || {
                let counter = Arc::clone(&alive_counter);
                async move {
                    counter.fetch_add(1, AtomicOrdering::SeqCst);
                    Json(json!({"id": "resp_failover", "model": "gpt-5.6", "output": []}))
                }
            }));
            let (alive_address, alive_task) = start_mock_upstream(alive).await;

            let targets = vec![
                UpstreamTarget::with_details(
                    "dead",
                    "Dead",
                    format!("http://{dead_address}/v1"),
                    "",
                    Vec::new(),
                    false,
                    false,
                )
                .unwrap(),
                UpstreamTarget::with_details(
                    "alive",
                    "Alive",
                    format!("http://{alive_address}/v1"),
                    "",
                    Vec::new(),
                    false,
                    false,
                )
                .unwrap(),
            ];
            let database_path = temp_database_path();
            let router = Arc::new(LocalRouterState::with_database_path(database_path.clone()));
            let info = router
                .start(RouterRuntimeConfig::new(
                    "127.0.0.1",
                    unused_port(),
                    true,
                    failover_upstreams(RouterAgent::Codex, targets, &["dead", "alive"]),
                ))
                .await
                .unwrap();

            const REQUESTS: usize = 32;
            let client = reqwest::Client::new();
            let url = format!("{}/codex/v1/responses", info.base_url);
            let mut handles = Vec::with_capacity(REQUESTS);
            for _ in 0..REQUESTS {
                let client = client.clone();
                let url = url.clone();
                handles.push(tokio::spawn(async move {
                    client
                        .post(url)
                        .json(&json!({"model": "gpt-5.6", "input": "hi"}))
                        .send()
                        .await
                        .map(|response| response.status())
                }));
            }
            for handle in handles {
                assert_eq!(
                    handle.await.unwrap().unwrap(),
                    StatusCode::OK,
                    "第一顺位挂了也必须转移成功"
                );
            }

            let status = wait_until_settled(&router, REQUESTS as u64).await;
            assert_eq!(status.total_requests, REQUESTS as u64);
            assert_eq!(
                status.successful_requests, REQUESTS as u64,
                "转移后成功的请求必须全部计成功"
            );
            assert_eq!(
                status.failed_requests, 0,
                "**转移成功不是请求失败** —— 计成失败会让失败率虚高,用户以为路由坏了"
            );

            let alive_served = alive_hits.load(AtomicOrdering::SeqCst);
            let dead_served = dead_hits.load(AtomicOrdering::SeqCst);
            assert_eq!(alive_served, REQUESTS, "每个请求最终都要落到健康那个");
            // 熔断器会在死目标失败够次数后拦下后续尝试,所以打到它的次数只能保证
            // "至少一次、不超过请求数" —— 不写死具体值,免得改了阈值就假红。
            assert!(
                (1..=REQUESTS).contains(&dead_served),
                "打到死目标的次数应在 1..={REQUESTS},实际 {dead_served}"
            );

            let recent = router.recent_requests(REQUESTS * 2).await.unwrap();
            assert_eq!(recent.len(), REQUESTS);
            assert!(recent.iter().all(|row| row.success));
            assert!(
                recent
                    .iter()
                    .all(|row| row.target_id.as_deref() == Some("alive")),
                "落库的目标必须是最终服务的那个,不是最初选中的"
            );

            router.stop().await.unwrap();
            dead_task.abort();
            alive_task.abort();
            let _ = dead_task.await;
            let _ = alive_task.await;
            remove_database(&database_path);
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
        async fn concurrent_upstream_failures_are_all_accounted_for() {
            // 只有一个目标且它一直 500(不可重试的对面故障)。32 路并发下
            // 失败计数必须**正好** 32:少了就是有失败丢在竞态里,而"最近错误"
            // 横幅会显示成偶发,排查时会被带偏。
            const REQUESTS: usize = 32;
            let upstream = Router::new().fallback(any(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": {"message": "upstream exploded"}})),
                )
            }));
            let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;

            let database_path = temp_database_path();
            let router = Arc::new(LocalRouterState::with_database_path(database_path.clone()));
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
            let info = router
                .start(RouterRuntimeConfig::new(
                    "127.0.0.1",
                    unused_port(),
                    true,
                    runtime_with_target(
                        RouterAgent::Codex,
                        target,
                        RouterAgentPolicy {
                            active_target: "only".to_string(),
                            ..RouterAgentPolicy::default()
                        },
                    ),
                ))
                .await
                .unwrap();

            let client = reqwest::Client::new();
            let url = format!("{}/codex/v1/responses", info.base_url);
            let mut handles = Vec::with_capacity(REQUESTS);
            for _ in 0..REQUESTS {
                let client = client.clone();
                let url = url.clone();
                handles.push(tokio::spawn(async move {
                    client
                        .post(url)
                        .json(&json!({"model": "gpt-5.6", "input": "hi"}))
                        .send()
                        .await
                        .map(|response| response.status())
                }));
            }
            for handle in handles {
                assert_eq!(
                    handle.await.unwrap().unwrap(),
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "上游的状态码必须原样透出,不能被本地改写"
                );
            }

            let status = wait_until_settled(&router, REQUESTS as u64).await;
            assert_eq!(status.active_requests, 0);
            assert_eq!(status.total_requests, REQUESTS as u64);
            assert_eq!(status.successful_requests, 0);
            assert_eq!(
                status.failed_requests, REQUESTS as u64,
                "32 个失败一个都不能丢"
            );
            let last_error = status.last_error.expect("真实故障必须留下错误横幅");
            assert_eq!(last_error.agent, Some(RouterAgent::Codex));
            assert!(
                last_error.message.contains("upstream exploded"),
                "错误横幅要带上游给的原因,实际 {:?}",
                last_error.message
            );

            let recent = router.recent_requests(REQUESTS * 2).await.unwrap();
            assert_eq!(recent.len(), REQUESTS);
            assert!(recent.iter().all(|row| !row.success));
            assert!(recent.iter().all(|row| row.status_code == 500));

            router.stop().await.unwrap();
            upstream_task.abort();
            let _ = upstream_task.await;
            remove_database(&database_path);
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
        async fn clients_walking_away_mid_stream_do_not_pollute_the_failure_rate() {
            // **这一条钉的是 `CLIENT_ABORT_SUMMARIES` 那段注释描述的语义**:
            // 用户按 Esc 打断、关掉任务,走的是 `ProxyBodyState::drop` /
            // `RequestCompletion::drop`,属于正常操作而非路由故障 ——
            // 计进失败率或错误横幅,用户每打断一次就看到一条红色报错。
            //
            // 造法:上游发一个 SSE 分片后**持续慢速发**,客户端只读到响应头就把
            // Response 丢掉(连接关闭),于是 body 那侧走 Drop。
            // 持续发而不是挂住,是为了让 hyper 在写入时立刻发现对端已关,
            // 而不是把 body future 停在那里等到超时 —— 后者会让这条用例变慢且飘。
            const REQUESTS: usize = 6;
            let upstream = Router::new().fallback(any(|| async {
                let chunks = stream::unfold(0_usize, |index| async move {
                    if index > 0 {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    let chunk = Bytes::from(format!(
                        "event: message_delta\ndata: {{\"type\":\"message_delta\",\"seq\":{index}}}\n\n"
                    ));
                    Some((Ok::<Bytes, Infallible>(chunk), index + 1))
                });
                let mut response = Response::new(Body::from_stream(chunks));
                response
                    .headers_mut()
                    .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
                response
            }));
            let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;

            let database_path = temp_database_path();
            let router = Arc::new(LocalRouterState::with_database_path(database_path.clone()));
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

            let url = format!("{}/claude/v1/messages", info.base_url);
            for _ in 0..REQUESTS {
                // 每次用独立 client:连接池复用会让"丢掉 Response"变成把连接还池,
                // 对端未必立刻感知关闭。独立 client 出作用域即关连接。
                let client = reqwest::Client::new();
                let response = client
                    .post(&url)
                    .json(&json!({"model": "claude-sonnet-4-5", "stream": true}))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                drop(response);
                drop(client);
            }

            let status = wait_until_settled(&router, REQUESTS as u64).await;
            assert_eq!(status.active_requests, 0, "断开也必须回收在途计数");
            assert_eq!(status.total_requests, REQUESTS as u64);
            assert_eq!(
                status.failed_requests, 0,
                "**客户端断开不算失败** —— 否则用户每次打断都拉高失败率"
            );
            assert_eq!(status.successful_requests, 0, "断开也不算成功,它两边都不站");
            assert_eq!(
                status.last_error, None,
                "**断开不能写错误横幅** —— 这是 CLIENT_ABORT_SUMMARIES 存在的全部理由"
            );

            // 请求行仍要落库(方便在历史里排查),但标成不成功且带断开原因。
            let recent = wait_for_rows(&router, REQUESTS).await;
            assert_eq!(recent.len(), REQUESTS, "断开的请求同样要留下历史");
            assert!(recent.iter().all(|row| !row.success));
            for row in &recent {
                let summary = row.error_summary.as_deref().unwrap_or_default();
                // **必须钉到具体那一条,不能只断言"是名单里的某一条"。**
                // 流式响应的记账守卫是套着的:`RequestCompletion` 装在
                // `ProxyBodyState.stream_completion` 里。把 `ProxyBodyState::drop`
                // 打哑之后,里面那个 `RequestCompletion` 随之自然析构,于是它自己的
                // `Drop` 接手、写下另一条文案("...was cancelled") —— 计数照样平,
                // 松断言照样绿。实测:变异掉 `ProxyBodyState::drop` 时这条用例存活。
                // 差别在于那条路径**不会**调 `release_neutral`,半开令牌就漏了。
                assert_eq!(
                    summary, CLIENT_ABORT_SUMMARIES[0],
                    "流式断开必须由 ProxyBodyState::drop 收尾(它才会归还半开令牌),实际 {summary:?}"
                );
            }

            router.stop().await.unwrap();
            upstream_task.abort();
            let _ = upstream_task.await;
            remove_database(&database_path);
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
        async fn a_request_cancelled_before_the_upstream_answers_still_balances_the_ledger() {
            // 上一条走的是**流式** body 那层守卫(`ProxyBodyState::drop`)。
            // 这一条走**外层**那个:客户端在上游还没回话时就走了,整个 handler future
            // 被 axum 丢掉,`complete()` 压根没机会跑 —— 收尾只能靠
            // `RequestCompletion::drop`。它是 `active_requests` 唯一的兜底:
            // 少了它,每一次"用户还没等到回话就切走"都会把在途数永久加一,
            // 健康面板上攒出一堆永不消失的在途请求。
            const REQUESTS: usize = 4;
            let upstream = Router::new().fallback(any(|| async {
                // 比客户端的等待时间长得多:确保断开发生在上游回话之前。
                tokio::time::sleep(Duration::from_secs(30)).await;
                Json(json!({"id": "never-delivered"}))
            }));
            let (upstream_address, upstream_task) = start_mock_upstream(upstream).await;

            let database_path = temp_database_path();
            let router = Arc::new(LocalRouterState::with_database_path(database_path.clone()));
            let target = UpstreamTarget::with_details(
                "codex",
                "Codex",
                format!("http://{upstream_address}/v1"),
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

            let url = format!("{}/codex/v1/responses", info.base_url);
            for _ in 0..REQUESTS {
                let client = reqwest::Client::new();
                // 给一个够短的超时,让 reqwest 自己放弃并关连接。
                let outcome = client
                    .post(&url)
                    .timeout(Duration::from_millis(300))
                    .json(&json!({"model": "gpt-5.6", "input": "hi"}))
                    .send()
                    .await;
                assert!(outcome.is_err(), "上游挂着 30 秒,客户端只能超时");
                drop(client);
            }

            let status = wait_until_settled(&router, REQUESTS as u64).await;
            assert_eq!(
                status.active_requests, 0,
                "**取消也必须回收在途** —— 这是 RequestCompletion::drop 唯一的职责"
            );
            assert_eq!(status.total_requests, REQUESTS as u64);
            assert_eq!(status.failed_requests, 0, "取消不算失败");
            assert_eq!(status.successful_requests, 0, "取消也不算成功");
            assert_eq!(status.last_error, None, "取消不写错误横幅");

            let recent = wait_for_rows(&router, REQUESTS).await;
            for row in &recent {
                let summary = row.error_summary.as_deref().unwrap_or_default();
                // 同样钉到具体那一条:这条路径只可能由外层守卫写下。
                assert_eq!(
                    summary, CLIENT_ABORT_SUMMARIES[1],
                    "上游回话前取消必须由 RequestCompletion::drop 收尾,实际 {summary:?}"
                );
                assert!(!row.success);
            }

            router.stop().await.unwrap();
            upstream_task.abort();
            let _ = upstream_task.await;
            remove_database(&database_path);
        }
    }
}
