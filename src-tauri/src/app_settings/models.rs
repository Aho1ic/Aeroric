use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelDetectionPolicy {
    LocalUser,
    PairedDevice,
}

pub(super) fn is_private_or_local_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || address.is_multicast()
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            address
                .to_ipv4()
                .map(|mapped| is_private_or_local_ip(IpAddr::V4(mapped)))
                .unwrap_or(false)
                || address.is_loopback()
                || address.is_unspecified()
                || address.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
        }
    }
}

pub(super) async fn resolve_remote_model_addresses(
    base_url: &url::Url,
) -> Result<Vec<SocketAddr>, String> {
    let host = base_url
        .host_str()
        .ok_or_else(|| "Base URL must include a host".to_string())?;
    let port = base_url
        .port_or_known_default()
        .ok_or_else(|| "Base URL must include a supported port".to_string())?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| format!("Could not resolve model endpoint: {error}"))?
        .filter(|address| !is_private_or_local_ip(address.ip()))
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err("Remote model detection cannot target a local or private address".to_string());
    }
    Ok(addresses)
}

pub(super) fn validate_model_base_url(
    base_url: &str,
    policy: ModelDetectionPolicy,
) -> Result<url::Url, String> {
    let normalized = normalize_base_url(base_url);
    if normalized.is_empty() {
        return Err("Base URL is required".to_string());
    }
    let url = url::Url::parse(&normalized).map_err(|_| "Invalid Base URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Base URL must use http or https".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Base URL cannot contain credentials".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "Base URL must include a host".to_string())?;
    if matches!(policy, ModelDetectionPolicy::PairedDevice) {
        let normalized_host = host.trim_end_matches('.').to_ascii_lowercase();
        let local_name = normalized_host == "localhost"
            || normalized_host.ends_with(".localhost")
            || normalized_host.ends_with(".local");
        let private_ip = normalized_host
            .parse::<IpAddr>()
            .map(is_private_or_local_ip)
            .unwrap_or(false);
        if local_name || private_ip {
            return Err(
                "Remote model detection cannot target a local or private address".to_string(),
            );
        }
    }
    Ok(url)
}

pub(super) fn model_endpoint(base_url: &url::Url) -> String {
    let mut endpoint = base_url.clone();
    let mut path = endpoint.path().trim_end_matches('/').to_string();
    if !path.ends_with("/v1") {
        path.push_str("/v1");
    }
    path.push_str("/models");
    endpoint.set_path(&path);
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    endpoint.to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AgentModelAuth {
    Bearer,
    BearerAndApiKey,
    ApiKey,
}

pub(super) fn model_auth_attempts(kind: &AgentSetupKind) -> [AgentModelAuth; 3] {
    match kind {
        AgentSetupKind::Codex => [
            AgentModelAuth::Bearer,
            AgentModelAuth::BearerAndApiKey,
            AgentModelAuth::ApiKey,
        ],
        AgentSetupKind::ClaudeCode => [
            AgentModelAuth::BearerAndApiKey,
            AgentModelAuth::ApiKey,
            AgentModelAuth::Bearer,
        ],
    }
}

pub(super) fn apply_model_auth(
    request: reqwest::RequestBuilder,
    api_key: &str,
    auth: AgentModelAuth,
) -> reqwest::RequestBuilder {
    match auth {
        AgentModelAuth::Bearer => request.bearer_auth(api_key),
        AgentModelAuth::BearerAndApiKey => request
            .bearer_auth(api_key)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        AgentModelAuth::ApiKey => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
    }
}

pub(super) async fn fetch_agent_model_json(
    client: &reqwest::Client,
    endpoint: &str,
    kind: &AgentSetupKind,
    api_key: &str,
) -> Result<serde_json::Value, String> {
    let attempts = model_auth_attempts(kind);
    for (index, auth) in attempts.into_iter().enumerate() {
        let response = apply_model_auth(client.get(endpoint), api_key, auth)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if status.is_success() {
            return response
                .json::<serde_json::Value>()
                .await
                .map_err(|e| e.to_string());
        }

        let can_retry_auth = matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) && index + 1 < attempts.len();
        if !can_retry_auth {
            return Err(format!("Model detection failed: HTTP {}", status));
        }
    }

    Err("Model detection failed".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AgentBalanceProvider {
    OpenRouter,
}

pub(super) fn balance_provider(base_url: &str) -> Option<AgentBalanceProvider> {
    let url = url::Url::parse(base_url.trim()).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if host == "openrouter.ai" {
        return Some(AgentBalanceProvider::OpenRouter);
    }
    None
}

pub(super) fn balance_endpoint(base_url: &str, provider: AgentBalanceProvider) -> Option<String> {
    let mut url = url::Url::parse(base_url.trim()).ok()?;
    url.set_query(None);
    url.set_fragment(None);
    url.set_path(match provider {
        AgentBalanceProvider::OpenRouter => "/api/v1/key",
    });
    Some(url.to_string())
}

pub(super) fn api_root_endpoint(base_url: &str, suffix: &str) -> Option<String> {
    let mut url = url::Url::parse(base_url.trim()).ok()?;
    let mut root = url.path().trim_end_matches('/').to_string();
    if root == "/v1" {
        root.clear();
    } else if root.ends_with("/v1") {
        root.truncate(root.len() - 3);
    }
    url.set_query(None);
    url.set_fragment(None);
    url.set_path(&format!("{}{}", root.trim_end_matches('/'), suffix));
    Some(url.to_string())
}

pub(super) fn parse_balance_number(value: &serde_json::Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())?;
    number.is_finite().then_some(number)
}

pub(super) fn parse_agent_balance(
    provider: AgentBalanceProvider,
    value: &serde_json::Value,
) -> Option<AgentBalance> {
    let (used, total) = match provider {
        AgentBalanceProvider::OpenRouter => {
            let usage = value.pointer("/data/usage").and_then(parse_balance_number);
            let total = value.pointer("/data/limit").and_then(parse_balance_number);
            let remaining = value
                .pointer("/data/limit_remaining")
                .and_then(parse_balance_number);
            let used = usage.or_else(|| {
                total
                    .zip(remaining)
                    .map(|(total, remaining)| total - remaining)
            })?;
            (used, total)
        }
    };

    (used.is_finite() && total.is_none_or(|total| total.is_finite() && total >= 0.0) && used >= 0.0)
        .then_some(AgentBalance { used, total })
}

pub(super) fn parse_new_api_token_balance(value: &serde_json::Value) -> Option<AgentBalance> {
    let data = value.get("data")?;
    let used = parse_balance_number(data.get("total_used")?)?;
    let unlimited = data
        .get("unlimited_quota")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let total = if unlimited {
        None
    } else {
        Some(parse_balance_number(data.get("total_granted")?)?)
    };

    (used >= 0.0 && total.is_none_or(|total| total >= 0.0)).then_some(AgentBalance { used, total })
}

pub(super) fn parse_dashboard_balance(
    subscription: &serde_json::Value,
    usage: &serde_json::Value,
) -> Option<AgentBalance> {
    let used = parse_balance_number(usage.get("total_usage")?)? / 100.0;
    let total = parse_balance_number(
        subscription
            .get("hard_limit_usd")
            .or_else(|| subscription.get("system_hard_limit_usd"))
            .or_else(|| subscription.get("soft_limit_usd"))?,
    )?;
    let total = (total < 100_000_000.0).then_some(total);

    (used.is_finite() && used >= 0.0 && total.is_none_or(|total| total >= 0.0))
        .then_some(AgentBalance { used, total })
}

pub(super) async fn get_balance_json(
    client: &reqwest::Client,
    endpoint: String,
    api_key: &str,
) -> Option<serde_json::Value> {
    let response = client
        .get(endpoint)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json::<serde_json::Value>().await.ok()
}

pub(super) async fn fetch_agent_balance(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Option<AgentBalance> {
    if let Some(provider) = balance_provider(base_url) {
        let endpoint = balance_endpoint(base_url, provider)?;
        let value = get_balance_json(client, endpoint, api_key).await?;
        return parse_agent_balance(provider, &value);
    }

    let token_endpoint = api_root_endpoint(base_url, "/api/usage/token/");
    if let Some(endpoint) = token_endpoint {
        if let Some(value) = get_balance_json(client, endpoint, api_key).await {
            if let Some(balance) = parse_new_api_token_balance(&value) {
                return Some(balance);
            }
        }
    }

    let subscription_endpoint = api_root_endpoint(base_url, "/dashboard/billing/subscription");
    let usage_endpoint = api_root_endpoint(base_url, "/dashboard/billing/usage");
    let (subscription, usage) = match (subscription_endpoint, usage_endpoint) {
        (Some(subscription), Some(usage)) => tokio::join!(
            get_balance_json(client, subscription, api_key),
            get_balance_json(client, usage, api_key)
        ),
        _ => return None,
    };
    parse_dashboard_balance(&subscription?, &usage?)
}

pub(super) fn looks_like_model_id(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/' | ':'))
}

pub(super) fn push_model_id(out: &mut Vec<String>, value: &str) {
    let model = value.trim();
    if looks_like_model_id(model) && !out.iter().any(|existing| existing == model) {
        out.push(model.to_string());
    }
}

pub(super) fn collect_model_ids(value: &serde_json::Value, out: &mut Vec<String>) {
    if let Some(id) = value.as_str() {
        push_model_id(out, id);
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            collect_model_ids(item, out);
        }
        return;
    }

    let Some(object) = value.as_object() else {
        return;
    };

    if object
        .get("visibility")
        .and_then(|visibility| visibility.as_str())
        .is_some_and(|visibility| visibility.eq_ignore_ascii_case("hidden"))
    {
        return;
    }

    for key in ["id", "name", "slug", "model", "display_name"] {
        if let Some(id) = object.get(key).and_then(|id| id.as_str()) {
            push_model_id(out, id);
        }
    }

    for key in ["data", "models", "items"] {
        let Some(nested) = object.get(key) else {
            continue;
        };
        if let Some(map) = nested.as_object() {
            for (model_key, model_value) in map {
                push_model_id(out, model_key);
                collect_model_ids(model_value, out);
            }
        } else {
            collect_model_ids(nested, out);
        }
    }
}

pub(super) fn parse_model_ids(value: serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_model_ids(&value, &mut out);
    out.sort_by_key(|model| model.to_ascii_lowercase());
    out.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    out
}

pub(super) fn parse_codex_model_catalog(value: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(value).map_err(|e| e.to_string())?;
    Ok(parse_model_ids(value))
}

pub(super) fn claude_builtin_model_aliases() -> Vec<String> {
    CLAUDE_BUILTIN_MODEL_ALIASES
        .iter()
        .map(|model| (*model).to_string())
        .collect()
}

pub(super) fn list_builtin_claude_models() -> Vec<String> {
    claude_builtin_model_aliases()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_style_model_ids() {
        let value = serde_json::json!({
            "data": [
                { "id": "z-model" },
                { "id": "a-model" },
                { "id": "a-model" }
            ]
        });

        assert_eq!(parse_model_ids(value), vec!["a-model", "z-model"]);
    }

    #[test]
    fn retries_model_detection_with_compatible_auth_headers() {
        assert_eq!(
            model_auth_attempts(&AgentSetupKind::Codex),
            [
                AgentModelAuth::Bearer,
                AgentModelAuth::BearerAndApiKey,
                AgentModelAuth::ApiKey,
            ]
        );
        assert_eq!(
            model_auth_attempts(&AgentSetupKind::ClaudeCode),
            [
                AgentModelAuth::BearerAndApiKey,
                AgentModelAuth::ApiKey,
                AgentModelAuth::Bearer,
            ]
        );
    }

    #[tokio::test]
    async fn retries_unauthorized_model_requests_with_api_key_headers() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}/v1/models", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 2048];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let count = stream.read(&mut chunk).unwrap();
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..count]);
                }
                let request = String::from_utf8_lossy(&request).to_ascii_lowercase();

                if attempt == 0 {
                    assert!(request.contains("authorization: bearer sk-test"));
                    assert!(!request.contains("x-api-key:"));
                    stream
                        .write_all(
                            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                } else {
                    assert!(request.contains("authorization: bearer sk-test"));
                    assert!(request.contains("x-api-key: sk-test"));
                    assert!(request.contains("anthropic-version: 2023-06-01"));
                    let body = br#"{"data":[{"id":"fallback-model"}]}"#;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .unwrap();
                    stream.write_all(body).unwrap();
                }
            }
        });

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let value = fetch_agent_model_json(&client, &endpoint, &AgentSetupKind::Codex, "sk-test")
            .await
            .unwrap();

        assert_eq!(parse_model_ids(value), vec!["fallback-model"]);
        server.join().unwrap();
    }
    #[test]
    fn detects_supported_balance_providers_and_endpoints() {
        let openrouter = "https://openrouter.ai/api/v1";

        assert_eq!(
            balance_provider(openrouter),
            Some(AgentBalanceProvider::OpenRouter)
        );
        assert_eq!(
            balance_endpoint(openrouter, AgentBalanceProvider::OpenRouter).as_deref(),
            Some("https://openrouter.ai/api/v1/key")
        );
        assert_eq!(
            api_root_endpoint("https://example.com/v1", "/api/usage/token/").as_deref(),
            Some("https://example.com/api/usage/token/")
        );
        assert_eq!(
            api_root_endpoint("https://example.com/codex/v1", "/api/usage/token/").as_deref(),
            Some("https://example.com/codex/api/usage/token/")
        );
        assert_eq!(balance_provider("https://example.com/v1"), None);
    }

    #[test]
    fn model_detection_url_policy_rejects_unsafe_remote_targets() {
        let public = validate_model_base_url(
            "https://api.example.com/v1/",
            ModelDetectionPolicy::PairedDevice,
        )
        .unwrap();
        assert_eq!(model_endpoint(&public), "https://api.example.com/v1/models");
        assert!(validate_model_base_url(
            "http://127.0.0.1:11434",
            ModelDetectionPolicy::PairedDevice,
        )
        .is_err());
        assert!(validate_model_base_url(
            "ftp://api.example.com",
            ModelDetectionPolicy::PairedDevice,
        )
        .is_err());
        assert!(validate_model_base_url(
            "https://user:pass@api.example.com",
            ModelDetectionPolicy::PairedDevice,
        )
        .is_err());
        assert!(
            validate_model_base_url("http://127.0.0.1:11434", ModelDetectionPolicy::LocalUser,)
                .is_ok()
        );
        assert!(is_private_or_local_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_private_or_local_ip("100.64.0.1".parse().unwrap()));
    }

    #[test]
    fn parses_openrouter_key_balance() {
        let value = serde_json::json!({
            "data": {
                "limit": 100,
                "limit_remaining": 42.75,
                "usage": 57.25
            }
        });

        assert_eq!(
            parse_agent_balance(AgentBalanceProvider::OpenRouter, &value),
            Some(AgentBalance {
                used: 57.25,
                total: Some(100.0),
            })
        );
        assert_eq!(
            parse_agent_balance(
                AgentBalanceProvider::OpenRouter,
                &serde_json::json!({
                    "data": {
                        "limit": null,
                        "limit_remaining": null,
                        "usage": 57.25
                    }
                })
            ),
            Some(AgentBalance {
                used: 57.25,
                total: None,
            })
        );
        assert_eq!(
            parse_agent_balance(
                AgentBalanceProvider::OpenRouter,
                &serde_json::json!({ "data": { "limit_remaining": null } })
            ),
            None
        );
    }

    #[test]
    fn parses_new_api_token_balance() {
        assert_eq!(
            parse_new_api_token_balance(&serde_json::json!({
                "code": true,
                "data": {
                    "total_granted": 100,
                    "total_used": 57.25,
                    "total_available": 42.75,
                    "unlimited_quota": false
                }
            })),
            Some(AgentBalance {
                used: 57.25,
                total: Some(100.0),
            })
        );
        assert_eq!(
            parse_new_api_token_balance(&serde_json::json!({
                "data": {
                    "total_granted": 0,
                    "total_used": 57.25,
                    "unlimited_quota": true
                }
            })),
            Some(AgentBalance {
                used: 57.25,
                total: None,
            })
        );
    }

    #[test]
    fn parses_one_api_dashboard_balance() {
        assert_eq!(
            parse_dashboard_balance(
                &serde_json::json!({ "hard_limit_usd": 100 }),
                &serde_json::json!({ "total_usage": 5725 })
            ),
            Some(AgentBalance {
                used: 57.25,
                total: Some(100.0),
            })
        );
    }

    #[test]
    fn parses_provider_model_names_from_common_catalog_shapes() {
        let value = serde_json::json!({
            "models": {
                "glm": { "name": "GLM" },
                "mimo": {},
                "claude-opus-4-6": { "id": "claude-opus-4-6" },
                "GLM-5.2": { "display_name": "GLM-5.2" }
            },
            "items": [
                { "model": "claude" }
            ]
        });

        assert_eq!(
            parse_model_ids(value),
            vec!["claude", "claude-opus-4-6", "glm", "GLM-5.2", "mimo"]
        );
    }

    #[test]
    fn parses_codex_model_catalog_slugs() {
        let value = serde_json::json!({
            "models": [
                { "slug": "gpt-5.6-sol", "visibility": "list" },
                { "slug": "hidden-model", "visibility": "hidden" },
                { "slug": "gpt-5.6-sol", "visibility": "list" },
                { "slug": "gpt-5.6-terra", "visibility": "list" },
                { "slug": "gpt-5.6-luna", "visibility": "list" }
            ]
        })
        .to_string();

        assert_eq!(
            parse_codex_model_catalog(&value).unwrap(),
            vec!["gpt-5.6-luna", "gpt-5.6-sol", "gpt-5.6-terra"]
        );
    }

    #[test]
    fn codex_model_dropdowns_only_use_reported_models() {
        let value = serde_json::json!({
            "models": [
                { "slug": "gpt-5.5", "visibility": "list" },
                { "slug": "gpt-5.4", "visibility": "list" }
            ]
        })
        .to_string();

        assert_eq!(
            parse_codex_model_catalog(&value).unwrap(),
            vec!["gpt-5.4", "gpt-5.5"]
        );
    }
}
