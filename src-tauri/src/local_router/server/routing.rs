//! 把一个进来的请求映射到某个上游 URL。
//!
//! 从 `server.rs` 整块搬出来,内容一行没改。这里只做**纯映射**:
//! 认出是哪个 agent、剥掉路径前缀、拼出目标 URL。不碰网络,不碰鉴权。

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SelectedRoute {
    pub(super) agent: RouterAgent,
    pub(super) forward_path: String,
    pub(super) target_id: Option<String>,
}

impl SelectedRoute {
    pub(super) fn bridges_responses_to_chat(&self, target: &UpstreamTarget) -> bool {
        if self.agent != RouterAgent::Codex || !target.enable_chat_completions_proxy() {
            return false;
        }
        matches!(
            self.forward_path.trim_end_matches('/'),
            "/responses" | "/v1/responses"
        )
    }

    pub(super) fn semantic_protocol(&self, target: &UpstreamTarget) -> Option<SemanticProtocol> {
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

pub(super) fn select_route(uri: &Uri, headers: &HeaderMap) -> Result<SelectedRoute, &'static str> {
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
    let (agent, forward_path, target_id) = if let Some((agent, path, target_id)) = prefixed {
        if marker.is_some_and(|marker| marker != agent) {
            return Err("local router path and agent marker disagree");
        }
        (agent, path, target_id)
    } else if path.starts_with("/v1/messages") {
        (RouterAgent::Claude, path.to_string(), None)
    } else if path.starts_with("/v1/responses")
        || path.starts_with("/responses")
        || path.starts_with("/v1/chat/completions")
        || path.starts_with("/chat/completions")
        || path.starts_with("/v1/models")
        || path == "/models"
    {
        (RouterAgent::Codex, path.to_string(), None)
    } else if let Some(agent) = marker {
        (agent, path.to_string(), None)
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
        target_id,
    })
}

pub(super) fn strip_agent_prefix(
    path: &str,
    prefix: &str,
    agent: RouterAgent,
) -> Option<(RouterAgent, String, Option<String>)> {
    let suffix = path.strip_prefix(prefix)?;
    if !suffix.is_empty() && !suffix.starts_with('/') {
        return None;
    }
    let suffix = suffix.strip_prefix('/').unwrap_or(suffix);
    let (target_id, forward_path) = suffix
        .strip_prefix("targets/")
        .and_then(|target| target.split_once('/'))
        .map(|(target_id, path)| {
            (
                (!target_id.is_empty()).then(|| target_id.to_string()),
                format!("/{path}"),
            )
        })
        .unwrap_or_else(|| {
            (
                None,
                if suffix.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{suffix}")
                },
            )
        });
    Some((agent, forward_path, target_id))
}

pub(super) fn normalize_codex_path(path: &str) -> String {
    path.strip_prefix("/v1/v1/")
        .map(|suffix| format!("/v1/{suffix}"))
        .unwrap_or_else(|| path.to_string())
}

pub(super) fn build_upstream_url(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_router::UpstreamTarget;

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
        assert_eq!(route.target_id, None);

        let request = Request::builder()
            .uri("/codex/targets/codex-team/v1/responses")
            .body(Body::empty())
            .unwrap();
        let route = select_route(request.uri(), request.headers()).unwrap();
        assert_eq!(route.agent, RouterAgent::Codex);
        assert_eq!(route.forward_path, "/v1/responses");
        assert_eq!(route.target_id.as_deref(), Some("codex-team"));

        let request = Request::builder()
            .uri("/claude/targets/claude-team/v1/messages")
            .body(Body::empty())
            .unwrap();
        let route = select_route(request.uri(), request.headers()).unwrap();
        assert_eq!(route.agent, RouterAgent::Claude);
        assert_eq!(route.forward_path, "/v1/messages");
        assert_eq!(route.target_id.as_deref(), Some("claude-team"));

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
}
