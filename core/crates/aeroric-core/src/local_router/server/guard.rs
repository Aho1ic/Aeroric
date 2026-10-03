//! 请求进门时的安全闸门 + 头部过滤。
//!
//! 从 `server.rs` 整块搬出来,内容一行没改。单独成文件是因为这里全是
//! 安全相关的判断 —— 常量时间比较密钥、跨站请求识别、把路由自己的凭据
//! 从转发出去的头里剥掉、以及 hop-by-hop 头的清理。改这里要格外小心。

use super::*;

pub(super) fn filter_request_headers(headers: HeaderMap) -> HeaderMap {
    let mut filtered = filter_hop_by_hop(headers);
    filtered.remove(HOST);
    filtered.remove(CONTENT_LENGTH);
    filtered.remove(ROUTE_AGENT_HEADER);
    filtered.remove(ROUTER_TOKEN_HEADER);
    filtered
}

pub(super) fn request_is_authorized(config: &RouterRuntimeConfig, headers: &HeaderMap) -> bool {
    let Ok(listen_addr) =
        crate::local_router::validate_listen_address(&config.listen_address, config.port)
    else {
        return false;
    };
    if listen_addr.ip().is_loopback() {
        return true;
    }

    router_credentials(headers)
        .iter()
        .any(|credential| constant_time_secret_eq(credential, &config.access_token))
}

/// 判断请求是否来自浏览器的跨站上下文。
///
/// 绑定回环地址时 `request_is_authorized` 会放行不带 token 的请求 —— 本机的 agent CLI
/// 需要这个（它们并不知道 router token），但这同时意味着任意网页里的
/// `fetch("http://127.0.0.1:<port>/v1/messages")` 都能借用户的额度和上游凭据。
/// CLI 客户端不会带 `Origin` / `Sec-Fetch-Site`，而浏览器一定会带，据此把两者分开。
pub(super) fn request_is_cross_site(headers: &HeaderMap) -> bool {
    // Fetch Metadata 由浏览器强制写入，页面脚本无法伪造。
    if let Some(site) = trimmed_header(headers, SEC_FETCH_SITE_HEADER) {
        if site.eq_ignore_ascii_case("cross-site") {
            return true;
        }
    }
    // 没有 Fetch Metadata 的旧浏览器仍会为跨源请求带上 Origin。无法解析的取值
    // （典型是 sandbox iframe / file:// 页面的 `null`）按不可信处理。
    match trimmed_header(headers, ORIGIN.as_str()) {
        Some(origin) => !origin_is_loopback(origin),
        None => false,
    }
}

pub(super) fn trimmed_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn origin_is_loopback(origin: &str) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        // 只认 `localhost` 本身；`evil.localhost` 之类的子域不算本机来源。
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

pub(super) fn router_credentials(headers: &HeaderMap) -> Vec<&str> {
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

pub(super) fn constant_time_secret_eq(provided: &str, expected: &str) -> bool {
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

pub(super) fn strip_router_credentials(headers: &mut HeaderMap, access_token: &str) {
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

pub(super) fn filter_response_headers(headers: HeaderMap) -> HeaderMap {
    filter_hop_by_hop(headers)
}

pub(super) fn filter_hop_by_hop(mut headers: HeaderMap) -> HeaderMap {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_router::{RouterUpstreams, ROUTE_AGENT_HEADER};
    use axum::http::HeaderValue;

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
    fn loopback_requests_remain_compatible_without_credentials() {
        let config =
            RouterRuntimeConfig::new("127.0.0.1", 43123, false, RouterUpstreams::default());
        assert!(request_is_authorized(&config, &HeaderMap::new()));
    }

    #[test]
    fn agent_cli_requests_are_not_treated_as_cross_site() {
        // CLI 客户端既不带 Origin 也不带 Fetch Metadata。
        assert!(!request_is_cross_site(&HeaderMap::new()));

        let mut direct = HeaderMap::new();
        direct.insert("sec-fetch-site", HeaderValue::from_static("none"));
        assert!(!request_is_cross_site(&direct));
    }

    #[test]
    fn browser_cross_site_requests_are_rejected() {
        let mut fetch_metadata = HeaderMap::new();
        fetch_metadata.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(request_is_cross_site(&fetch_metadata));

        // 旧浏览器没有 Fetch Metadata，但跨源请求一定带 Origin。
        let mut remote_origin = HeaderMap::new();
        remote_origin.insert(ORIGIN, HeaderValue::from_static("https://evil.example"));
        assert!(request_is_cross_site(&remote_origin));

        // 不透明来源（sandbox iframe / file:// 页面）同样不可信。
        let mut opaque_origin = HeaderMap::new();
        opaque_origin.insert(ORIGIN, HeaderValue::from_static("null"));
        assert!(request_is_cross_site(&opaque_origin));

        // `localhost` 的子域会解析到回环地址，但并不是本机来源。
        let mut lookalike = HeaderMap::new();
        lookalike.insert(ORIGIN, HeaderValue::from_static("http://evil.localhost"));
        assert!(request_is_cross_site(&lookalike));
    }

    #[test]
    fn local_web_clients_are_allowed() {
        for origin in [
            "http://localhost:1420",
            "http://127.0.0.1:43123",
            "http://[::1]:43123",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
            headers.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
            assert!(
                !request_is_cross_site(&headers),
                "expected {origin} to be allowed"
            );
        }
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
}
