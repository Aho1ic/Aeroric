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

/// Codex 内置 ImageGen 使用的 legacy Images API 端点。
///
/// 这两个端点既不是 Responses 也不是 Chat Completions,任何一侧的桥接都表达不了
/// 它们的协议,所以只能原样透传给上游。ImageGen 一旦引用已有图片(显式路径或最近
/// 生成的 N 张)就从 `generations` 切到 `edits`,两条都必须认。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CodexImagesEndpoint {
    Generations,
    Edits,
}

impl CodexImagesEndpoint {
    /// 认出 `forward_path` 是哪个 Images 端点。
    ///
    /// Codex 会带着 `/v1` 前缀发,某些配置还会拼出双 `/v1/v1`(`/codex/v1/...` 已在
    /// `strip_agent_prefix` 里剥成 `/v1/...`),所以最多剥两层 `/v1`。后缀按大小写
    /// 不敏感比,和上游对完整 URL 后缀的判定保持一致。
    pub(super) fn from_forward_path(path: &str) -> Option<Self> {
        let mut path = path.trim_end_matches('/');
        for _ in 0..2 {
            match path.strip_prefix("/v1") {
                Some(rest) if rest.starts_with('/') => path = rest,
                _ => break,
            }
        }
        if path.eq_ignore_ascii_case(Self::Generations.path()) {
            Some(Self::Generations)
        } else if path.eq_ignore_ascii_case(Self::Edits.path()) {
            Some(Self::Edits)
        } else {
            None
        }
    }

    pub(super) fn path(self) -> &'static str {
        match self {
            Self::Generations => "/images/generations",
            Self::Edits => "/images/edits",
        }
    }
}

/// 能推导出 Images 端点的完整 URL 后缀表。
///
/// 两条 Images 路由互为兄弟,所以任一条被粘成 base_url 都能推出另一条;
/// Responses / Compact / Chat Completions 与 Images 在上游同级,也能推。
/// `"/responses/compact"` 必须排在 `"/responses"` 之前,否则短的先命中会留下
/// 一截 `/compact`。
const IMAGES_SIBLING_SUFFIXES: &[&str] = &[
    "/images/generations",
    "/images/edits",
    "/chat/completions",
    "/responses/compact",
    "/responses",
];

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
        // Codex 内置 ImageGen 直接打裸端点(不带 `/codex` 前缀),这里认出裸路径、
        // `/v1`、双 `/v1/v1` 三种别名;`/codex/v1/...` 走上面的前缀分支。
        || CodexImagesEndpoint::from_forward_path(path).is_some()
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

/// base_url 的 path 结尾是某个同级端点时,把那截换成 `endpoint` 的路径。
///
/// 只认得出兄弟端点的形状才改写:认不出就返回 `None`,交回通用段拼接,绝不凭空猜。
fn rewrite_full_endpoint_base(base_path: &str, endpoint: CodexImagesEndpoint) -> Option<String> {
    let base_path = base_path.trim_end_matches('/');
    let lowercase = base_path.to_ascii_lowercase();
    let suffix = IMAGES_SIBLING_SUFFIXES
        .iter()
        .find(|suffix| lowercase.ends_with(**suffix))?;
    let prefix = base_path.get(..base_path.len() - suffix.len())?;
    Some(format!("{prefix}{}", endpoint.path()))
}

pub(super) fn build_upstream_url(
    target: &UpstreamTarget,
    request_path: &str,
    query: Option<&str>,
) -> Result<Url, &'static str> {
    let mut url = target.base_url().clone();

    // base_url 被粘成一条完整端点 URL(`.../v1/chat/completions`、`.../v1/responses`
    // 之类)时,Images 请求按段拼接会拼出 `.../chat/completions/v1/images/generations`
    // ——一条必然 404 的死路。Images 与 Responses / Chat Completions 在上游同级,
    // 所以把结尾那截换成本次要打的 Images 路径。base_url 不认得的形状照旧走下面的
    // 通用拼接,不做猜测。
    let images_endpoint_path = CodexImagesEndpoint::from_forward_path(request_path)
        .and_then(|endpoint| rewrite_full_endpoint_base(url.path(), endpoint));
    if let Some(path) = images_endpoint_path {
        url.set_path(&path);
        url.set_query(query);
        return Ok(url);
    }
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

    /// CC-4 命中侧:Codex ImageGen 的四种别名(裸、`/v1`、双 `/v1/v1`、`/codex/v1`)
    /// 以及大小写混写的端点后缀都要落到 Codex 上,并且规范化成单层 `/v1`。
    #[test]
    fn codex_images_endpoint_aliases_all_route_to_codex() {
        for (uri, expected_path) in [
            ("/images/generations", "/images/generations"),
            ("/v1/images/generations", "/v1/images/generations"),
            ("/v1/v1/images/generations", "/v1/images/generations"),
            ("/codex/v1/images/generations", "/v1/images/generations"),
            ("/images/edits", "/images/edits"),
            ("/v1/images/edits", "/v1/images/edits"),
            ("/v1/v1/images/edits", "/v1/images/edits"),
            ("/codex/v1/images/edits", "/v1/images/edits"),
            ("/v1/Images/Generations", "/v1/Images/Generations"),
            ("/v1/IMAGES/EDITS", "/v1/IMAGES/EDITS"),
        ] {
            let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
            let route = select_route(request.uri(), request.headers())
                .unwrap_or_else(|error| panic!("{uri} should route: {error}"));
            assert_eq!(route.agent, RouterAgent::Codex, "{uri}");
            assert_eq!(route.forward_path, expected_path, "{uri}");
        }

        let request = Request::builder()
            .uri("/codex/targets/images-team/v1/images/edits")
            .body(Body::empty())
            .unwrap();
        let route = select_route(request.uri(), request.headers()).unwrap();
        assert_eq!(route.forward_path, "/v1/images/edits");
        assert_eq!(route.target_id.as_deref(), Some("images-team"));
    }

    /// CC-4 不命中侧:只有这两条端点算 Images 透传。相邻的 Images 路由、多出来的
    /// 路径段、以及普通 Responses 端点都不能被误判。
    #[test]
    fn non_images_endpoints_are_not_treated_as_images_passthrough() {
        for path in [
            "/v1/images/variations",
            "/v1/images",
            "/v1/images/generations/foo",
            "/v1/responses",
            "/v1/chat/completions",
            "/v1/messages",
            "/v1/v1/v1/images/generations",
        ] {
            assert!(
                CodexImagesEndpoint::from_forward_path(path).is_none(),
                "{path} must not be an Images endpoint"
            );
        }

        // 未注册的 Images 邻居没有 agent 标记时仍然是 404,不会被 Codex 兜走。
        let request = Request::builder()
            .uri("/v1/images/variations")
            .body(Body::empty())
            .unwrap();
        assert!(select_route(request.uri(), request.headers()).is_err());

        // Images 请求永不桥接;同一个 target 上普通 Responses 请求照旧桥接。
        let bridging_target = UpstreamTarget::with_details(
            "chat",
            "Chat",
            "https://gateway.example.test/v1",
            "",
            Vec::new(),
            false,
            true,
        )
        .unwrap();
        // 两条 Images 端点都不能桥接。`edits` 尤其致命:桥接会把 data-URL 图片连同
        // `prompt` 一起改写成 `messages`,上游必然 400。
        for forward_path in ["/v1/images/generations", "/v1/images/edits"] {
            let images = SelectedRoute {
                agent: RouterAgent::Codex,
                forward_path: forward_path.to_string(),
                target_id: None,
            };
            assert!(
                !images.bridges_responses_to_chat(&bridging_target),
                "{forward_path} must not bridge"
            );
            assert_eq!(
                images.semantic_protocol(&bridging_target),
                None,
                "{forward_path}"
            );
        }
        let responses = SelectedRoute {
            agent: RouterAgent::Codex,
            forward_path: "/v1/responses".to_string(),
            target_id: None,
        };
        assert!(responses.bridges_responses_to_chat(&bridging_target));
        assert_eq!(
            responses.semantic_protocol(&bridging_target),
            Some(SemanticProtocol::ChatCompletions)
        );
    }

    /// base_url 被粘成完整端点时,Images 请求要落到同级的 Images 路由,而不是拼成
    /// `.../chat/completions/v1/images/generations`。
    #[test]
    fn images_requests_derive_the_sibling_endpoint_from_a_full_endpoint_base() {
        for (base_url, request_path, expected) in [
            (
                "https://gateway.example.test/v1/chat/completions",
                "/v1/images/generations",
                "https://gateway.example.test/v1/images/generations",
            ),
            (
                "https://gateway.example.test/v1/responses",
                "/v1/images/edits",
                "https://gateway.example.test/v1/images/edits",
            ),
            (
                "https://gateway.example.test/v1/responses/compact",
                "/images/generations",
                "https://gateway.example.test/v1/images/generations",
            ),
            (
                "https://gateway.example.test/v1/images/generations",
                "/v1/images/edits",
                "https://gateway.example.test/v1/images/edits",
            ),
            (
                "https://gateway.example.test/Gateway/v1/Chat/Completions",
                "/v1/images/generations",
                "https://gateway.example.test/Gateway/v1/images/generations",
            ),
        ] {
            let target = UpstreamTarget::new(base_url).unwrap();
            let url = build_upstream_url(&target, request_path, None).unwrap();
            assert_eq!(url.as_str(), expected, "{base_url} + {request_path}");
        }

        // 普通 base_url 不触发改写,照旧走通用段拼接(含 v1 去重与 ChatGPT 特例)。
        let target = UpstreamTarget::new("https://gateway.example.test/v1").unwrap();
        let url = build_upstream_url(&target, "/v1/images/generations", Some("trace=1")).unwrap();
        assert_eq!(
            url.as_str(),
            "https://gateway.example.test/v1/images/generations?trace=1"
        );

        let target = UpstreamTarget::new("https://chatgpt.com/backend-api/codex").unwrap();
        let url = build_upstream_url(&target, "/v1/images/generations", None).unwrap();
        assert_eq!(
            url.as_str(),
            "https://chatgpt.com/backend-api/codex/images/generations"
        );

        // 非 Images 请求打到完整端点 base_url 时行为不变,改写只对 Images 生效。
        let target =
            UpstreamTarget::new("https://gateway.example.test/v1/chat/completions").unwrap();
        let url = build_upstream_url(&target, "/v1/chat/completions", None).unwrap();
        assert_eq!(
            url.as_str(),
            "https://gateway.example.test/v1/chat/completions"
        );
    }
}
