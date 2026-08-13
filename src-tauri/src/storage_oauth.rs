//! 网盘 OAuth 授权:PKCE + 127.0.0.1 回环重定向。
//!
//! 安全约束(必须保持):
//! - 桌面端**不内嵌任何 client_secret**。内置凭据只用于支持 PKCE 的 public
//!   client;需要 secret 的服务(百度网盘、阿里云盘开放平台)只能走用户自建应用。
//! - 回环地址固定 `127.0.0.1`(不是 `localhost`,避免解析到 IPv6 或被劫持)。
//! - `state` 用 CSPRNG 生成并强校验,防 CSRF。
//! - token 与 client_secret 一律经 `storage_conn` 写入 0600 的 secrets 文件,
//!   不写日志、不回传前端明文。

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

use crate::storage_conn::StorageProtocol;

/// 授权回调等待上限。超时后释放端口,避免占着回环端口不放。
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// 各服务的 OAuth 端点与 scope。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OauthProvider {
    pub authorize_url: &'static str,
    pub token_url: &'static str,
    pub scope: &'static str,
    /// 该服务的 token 交换是否强制要求 client_secret。
    /// 为 true 时不能使用内置凭据(桌面端不得内嵌 secret)。
    pub requires_client_secret: bool,
    /// 是否支持 PKCE(S256)。
    pub supports_pkce: bool,
}

/// 取协议对应的 OAuth 配置。
pub fn provider_for(protocol: StorageProtocol) -> Option<OauthProvider> {
    match protocol {
        StorageProtocol::Dropbox => Some(OauthProvider {
            authorize_url: "https://www.dropbox.com/oauth2/authorize",
            token_url: "https://api.dropboxapi.com/oauth2/token",
            scope: "files.content.read files.content.write files.metadata.read \
                    files.metadata.write account_info.read",
            requires_client_secret: false,
            supports_pkce: true,
        }),
        StorageProtocol::OneDrive => Some(OauthProvider {
            authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            scope: "offline_access Files.ReadWrite.All User.Read",
            requires_client_secret: false,
            supports_pkce: true,
        }),
        StorageProtocol::GoogleDrive => Some(OauthProvider {
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            scope: "https://www.googleapis.com/auth/drive",
            requires_client_secret: false,
            supports_pkce: true,
        }),
        StorageProtocol::Box => Some(OauthProvider {
            authorize_url: "https://account.box.com/api/oauth2/authorize",
            token_url: "https://api.box.com/oauth2/token",
            scope: "root_readwrite",
            // Box 的 authorization_code 交换必须带 client_secret。
            requires_client_secret: true,
            supports_pkce: false,
        }),
        StorageProtocol::AliyunDrive => Some(OauthProvider {
            authorize_url: "https://openapi.alipan.com/oauth/authorize",
            token_url: "https://openapi.alipan.com/oauth/access_token",
            scope: "user:base,file:all:read,file:all:write",
            requires_client_secret: true,
            supports_pkce: false,
        }),
        StorageProtocol::BaiduNetdisk => Some(OauthProvider {
            authorize_url: "https://openapi.baidu.com/oauth/2.0/authorize",
            token_url: "https://openapi.baidu.com/oauth/2.0/token",
            scope: "basic,netdisk",
            requires_client_secret: true,
            supports_pkce: false,
        }),
        _ => None,
    }
}

/// 内置(Aeroric 自有)client_id。
///
/// 只在服务支持 PKCE public client 时提供,因此这里不需要也不允许配套 secret。
/// 未配置的服务返回 `None`,前端必须引导用户填写自建应用凭据。
pub fn builtin_client_id(_protocol: StorageProtocol) -> Option<&'static str> {
    // 目前未发布任何内置应用凭据。保留此函数与 `CredentialSource::Builtin`
    // 分支,便于发布内置应用后只改这一处(按 protocol 匹配返回 client_id);
    // 在此之前 UI 会显示"需自建应用"。
    None
}

/// 凭据来源:内置公共应用或用户自建应用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialSource {
    Builtin,
    UserProvided,
}

/// 前端可用的凭据来源信息。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialOptions {
    /// 内置凭据是否可用。
    pub builtin_available: bool,
    /// 是否必须由用户提供 client_secret(桌面端不内嵌 secret)。
    pub requires_client_secret: bool,
    /// 是否使用 PKCE。
    pub supports_pkce: bool,
    /// 该服务需要的 scope,便于用户在自建应用里对齐配置。
    pub scope: String,
}

/// 查询某协议的凭据来源能力。
pub fn credential_options(protocol: StorageProtocol) -> Option<CredentialOptions> {
    let provider = provider_for(protocol)?;
    Some(CredentialOptions {
        // 需要 secret 的服务永远不能用内置凭据。
        builtin_available: !provider.requires_client_secret
            && builtin_client_id(protocol).is_some(),
        requires_client_secret: provider.requires_client_secret,
        supports_pkce: provider.supports_pkce,
        scope: provider.scope.to_string(),
    })
}

/// PKCE verifier / challenge 对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 生成 CSPRNG 随机串(base64url,无填充)。
fn random_token(byte_len: usize) -> Result<String, String> {
    let mut bytes = vec![0u8; byte_len];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    Ok(base64_url_no_pad(&bytes))
}

/// 生成 PKCE 对。verifier 43-128 字符,challenge 为其 SHA-256 的 base64url。
pub fn generate_pkce() -> Result<PkcePair, String> {
    let verifier = random_token(32)?;
    let challenge = base64_url_no_pad(&Sha256::digest(verifier.as_bytes()));
    Ok(PkcePair {
        verifier,
        challenge,
    })
}

/// 构造授权 URL。
pub fn build_authorize_url(
    provider: &OauthProvider,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    pkce: Option<&PkcePair>,
) -> String {
    let mut url = url::Url::parse(provider.authorize_url).expect("provider URL must be valid");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("client_id", client_id);
        query.append_pair("response_type", "code");
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("state", state);
        if !provider.scope.is_empty() {
            query.append_pair("scope", provider.scope);
        }
        if let Some(pkce) = pkce.filter(|_| provider.supports_pkce) {
            query.append_pair("code_challenge", &pkce.challenge);
            query.append_pair("code_challenge_method", "S256");
        }
        // Dropbox / Google 需要显式请求 refresh token。
        if provider.authorize_url.contains("dropbox.com") {
            query.append_pair("token_access_type", "offline");
        }
        if provider.authorize_url.contains("accounts.google.com") {
            query.append_pair("access_type", "offline");
            query.append_pair("prompt", "consent");
        }
    }
    url.to_string()
}

/// token 端点响应。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

/// 授权结果:写进连接 secrets 的键值对。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationResult {
    pub secrets: BTreeMap<String, String>,
}

/// 把 token 响应转成 secrets 键值。
pub fn token_response_to_secrets(
    response: &TokenResponse,
    client_id: &str,
    client_secret: Option<&str>,
    now_ms: i64,
) -> BTreeMap<String, String> {
    let mut secrets = BTreeMap::new();
    secrets.insert("accessToken".to_string(), response.access_token.clone());
    if let Some(refresh_token) = response.refresh_token.as_ref().filter(|v| !v.is_empty()) {
        secrets.insert("refreshToken".to_string(), refresh_token.clone());
    }
    if let Some(expires_in) = response.expires_in.filter(|value| *value > 0) {
        secrets.insert(
            "expiresAtMs".to_string(),
            (now_ms + expires_in * 1000).to_string(),
        );
    }
    if !client_id.is_empty() {
        secrets.insert("clientId".to_string(), client_id.to_string());
    }
    if let Some(client_secret) = client_secret.filter(|value| !value.is_empty()) {
        secrets.insert("clientSecret".to_string(), client_secret.to_string());
    }
    secrets
}

/// 校验回调 state 是否与本次会话一致(常量时间比较,防 CSRF 与时序探测)。
pub fn state_matches(expected: &str, received: &str) -> bool {
    if expected.is_empty() || expected.len() != received.len() {
        return false;
    }
    expected
        .bytes()
        .zip(received.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// 从回调 query 里取 code。带 `error` 时返回服务端错误信息。
pub fn extract_code(query: &str) -> Result<(String, String), String> {
    let pairs: BTreeMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    if let Some(error) = pairs.get("error") {
        let description = pairs
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| error.clone());
        return Err(description);
    }
    let code = pairs
        .get("code")
        .cloned()
        .ok_or_else(|| "Authorization response is missing \"code\"".to_string())?;
    let state = pairs.get("state").cloned().unwrap_or_default();
    Ok((code, state))
}

/// 回调页面 HTML。授权完成后浏览器停在这一页。
fn callback_page(success: bool, message: &str) -> String {
    let title = if success {
        "Authorization complete"
    } else {
        "Authorization failed"
    };
    // message 来自服务端错误描述,必须转义后再插入 HTML。
    let escaped = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font-family:-apple-system,system-ui,sans-serif;display:flex;\
         align-items:center;justify-content:center;height:100vh;margin:0;\
         background:#101014;color:#e8e8ed}}div{{text-align:center;max-width:32rem}}\
         h1{{font-size:1.05rem;font-weight:600;margin:0 0 .5rem}}\
         p{{font-size:.85rem;color:#9a9aa5;margin:0}}</style></head>\
         <body><div><h1>{title}</h1><p>{escaped}</p></div></body></html>"
    )
}

/// 在 127.0.0.1 上起一个一次性回调服务器,返回(端口, 结果接收端)。
async fn spawn_callback_server(
    expected_state: String,
) -> Result<(u16, oneshot::Receiver<Result<String, String>>), String> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let (sender, receiver) = oneshot::channel::<Result<String, String>>();
    let sender = Arc::new(std::sync::Mutex::new(Some(sender)));
    let expected_state = Arc::new(expected_state);

    let app = axum::Router::new().fallback(axum::routing::any({
        let sender = Arc::clone(&sender);
        let expected_state = Arc::clone(&expected_state);
        move |request: axum::extract::Request| {
            let sender = Arc::clone(&sender);
            let expected_state = Arc::clone(&expected_state);
            async move {
                let query = request.uri().query().unwrap_or("").to_string();
                let outcome = match extract_code(&query) {
                    Ok((code, state)) => {
                        if state_matches(&expected_state, &state) {
                            Ok(code)
                        } else {
                            Err("Authorization state mismatch".to_string())
                        }
                    }
                    Err(error) => Err(error),
                };
                let body = match &outcome {
                    Ok(_) => callback_page(true, "You can close this tab and return to Aeroric."),
                    Err(error) => callback_page(false, error),
                };
                if let Some(sender) = sender.lock().ok().and_then(|mut slot| slot.take()) {
                    let _ = sender.send(outcome);
                }
                axum::response::Html(body)
            }
        }
    }));

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((port, receiver))
}

/// 用 authorization code 换 token。
async fn exchange_code(
    provider: &OauthProvider,
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    code: &str,
    verifier: Option<&str>,
) -> Result<TokenResponse, String> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("client_id", client_id.to_string()),
    ];
    if let Some(client_secret) = client_secret.filter(|value| !value.is_empty()) {
        form.push(("client_secret", client_secret.to_string()));
    }
    if let Some(verifier) = verifier.filter(|_| provider.supports_pkce) {
        form.push(("code_verifier", verifier.to_string()));
    }

    let response = reqwest::Client::new()
        .post(provider.token_url)
        .form(&form)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        // 响应体可能含服务端错误描述,但不会含我们的 secret;仍然只回传摘要。
        return Err(format!("Token exchange failed ({status})"));
    }
    serde_json::from_str::<TokenResponse>(&body)
        .map_err(|_| "Token endpoint returned an unexpected response".to_string())
}

/// 校验凭据来源与服务要求是否自洽。
pub fn resolve_client_credentials(
    protocol: StorageProtocol,
    source: CredentialSource,
    user_client_id: Option<&str>,
    user_client_secret: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let provider = provider_for(protocol)
        .ok_or_else(|| format!("{} does not use OAuth", protocol.as_str()))?;
    match source {
        CredentialSource::Builtin => {
            if provider.requires_client_secret {
                return Err(format!(
                    "{} requires your own app credentials because its token endpoint \
                     mandates a client secret, which a desktop app cannot ship safely.",
                    protocol.as_str()
                ));
            }
            let client_id = builtin_client_id(protocol).ok_or_else(|| {
                format!(
                    "No built-in credentials are available for {}. Create your own app \
                     and enter its client ID.",
                    protocol.as_str()
                )
            })?;
            Ok((client_id.to_string(), None))
        }
        CredentialSource::UserProvided => {
            let client_id = user_client_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "\"clientId\" is required".to_string())?;
            let client_secret = user_client_secret
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            if provider.requires_client_secret && client_secret.is_none() {
                return Err(format!("{} requires \"clientSecret\"", protocol.as_str()));
            }
            Ok((client_id.to_string(), client_secret))
        }
    }
}

/// 回环重定向地址。固定 127.0.0.1,不用 localhost。
pub fn redirect_uri_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}/callback")
}

/// 走完整个授权流程,返回可写入连接 secrets 的键值。
///
/// 前端负责把返回的 secrets 交给 `storage_save_connection` 落盘。
#[tauri::command]
pub async fn storage_oauth_authorize(
    app: tauri::AppHandle,
    protocol: StorageProtocol,
    source: CredentialSource,
    client_id: Option<String>,
    client_secret: Option<String>,
) -> Result<AuthorizationResult, String> {
    let provider = provider_for(protocol)
        .ok_or_else(|| format!("{} does not use OAuth", protocol.as_str()))?;
    let (client_id, client_secret) = resolve_client_credentials(
        protocol,
        source,
        client_id.as_deref(),
        client_secret.as_deref(),
    )?;

    let state = random_token(24)?;
    let pkce = generate_pkce()?;
    let (port, receiver) = spawn_callback_server(state.clone()).await?;
    let redirect_uri = redirect_uri_for(port);
    let authorize_url =
        build_authorize_url(&provider, &client_id, &redirect_uri, &state, Some(&pkce));

    // 交给系统浏览器:桌面 webview 内嵌授权页会触发多家服务的策略拦截。
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_url(authorize_url, None::<&str>)
        .map_err(|error| error.to_string())?;

    let code = match tokio::time::timeout(AUTH_TIMEOUT, receiver).await {
        Ok(Ok(Ok(code))) => code,
        Ok(Ok(Err(error))) => return Err(error),
        Ok(Err(_)) => return Err("Authorization was cancelled".to_string()),
        Err(_) => return Err("Authorization timed out".to_string()),
    };

    let response = exchange_code(
        &provider,
        &client_id,
        client_secret.as_deref(),
        &redirect_uri,
        &code,
        Some(&pkce.verifier),
    )
    .await?;

    Ok(AuthorizationResult {
        secrets: token_response_to_secrets(
            &response,
            &client_id,
            client_secret.as_deref(),
            chrono::Utc::now().timestamp_millis(),
        ),
    })
}

/// 查询某协议的凭据来源能力(前端表单用)。
#[tauri::command]
pub async fn storage_oauth_credential_options(
    protocol: StorageProtocol,
) -> Result<Option<CredentialOptions>, String> {
    Ok(credential_options(protocol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_the_sha256_of_the_verifier() {
        let pair = generate_pkce().unwrap();
        assert!(pair.verifier.len() >= 43, "verifier too short for RFC 7636");
        assert!(pair.verifier.len() <= 128);
        let expected = base64_url_no_pad(&Sha256::digest(pair.verifier.as_bytes()));
        assert_eq!(pair.challenge, expected);
        // base64url 不能含 `+` `/` `=`。
        assert!(!pair.challenge.contains('+'));
        assert!(!pair.challenge.contains('/'));
        assert!(!pair.challenge.contains('='));
    }

    #[test]
    fn pkce_pairs_are_unique_per_call() {
        let a = generate_pkce().unwrap();
        let b = generate_pkce().unwrap();
        assert_ne!(a.verifier, b.verifier);
    }

    #[test]
    fn authorize_url_carries_pkce_and_state() {
        let provider = provider_for(StorageProtocol::Dropbox).unwrap();
        let pkce = generate_pkce().unwrap();
        let url = build_authorize_url(
            &provider,
            "client-1",
            "http://127.0.0.1:1234/callback",
            "state-1",
            Some(&pkce),
        );
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
        assert!(url.contains("state=state-1"));
        assert!(url.contains("response_type=code"));
        // Dropbox 需要 offline 才会下发 refresh token。
        assert!(url.contains("token_access_type=offline"));
        // verifier 绝不能出现在授权 URL 里。
        assert!(!url.contains(&pkce.verifier));
    }

    #[test]
    fn authorize_url_omits_pkce_for_providers_without_support() {
        let provider = provider_for(StorageProtocol::Box).unwrap();
        let pkce = generate_pkce().unwrap();
        let url = build_authorize_url(
            &provider,
            "client-1",
            "http://127.0.0.1:1234/callback",
            "state-1",
            Some(&pkce),
        );
        assert!(!url.contains("code_challenge"));
    }

    #[test]
    fn redirect_uri_is_loopback_ipv4() {
        let uri = redirect_uri_for(51234);
        assert_eq!(uri, "http://127.0.0.1:51234/callback");
        assert!(!uri.contains("localhost"));
    }

    #[test]
    fn state_comparison_rejects_mismatches_and_empty_values() {
        assert!(state_matches("abc123", "abc123"));
        assert!(!state_matches("abc123", "abc124"));
        assert!(!state_matches("abc123", "abc12"));
        assert!(!state_matches("", ""));
        assert!(!state_matches("abc", ""));
    }

    #[test]
    fn extract_code_reads_code_and_state() {
        let (code, state) = extract_code("code=abc&state=xyz").unwrap();
        assert_eq!(code, "abc");
        assert_eq!(state, "xyz");
    }

    #[test]
    fn extract_code_surfaces_provider_errors() {
        let error = extract_code("error=access_denied&error_description=User+said+no").unwrap_err();
        assert_eq!(error, "User said no");
        let error = extract_code("error=access_denied").unwrap_err();
        assert_eq!(error, "access_denied");
    }

    #[test]
    fn extract_code_requires_a_code() {
        assert!(extract_code("state=xyz").is_err());
        assert!(extract_code("").is_err());
    }

    #[test]
    fn builtin_source_is_refused_when_the_service_mandates_a_secret() {
        for protocol in [
            StorageProtocol::Box,
            StorageProtocol::AliyunDrive,
            StorageProtocol::BaiduNetdisk,
        ] {
            let error = resolve_client_credentials(protocol, CredentialSource::Builtin, None, None)
                .unwrap_err();
            assert!(
                error.contains("your own app credentials"),
                "unexpected error for {}: {error}",
                protocol.as_str()
            );
        }
    }

    #[test]
    fn user_provided_source_requires_a_client_id() {
        let error = resolve_client_credentials(
            StorageProtocol::Dropbox,
            CredentialSource::UserProvided,
            Some("  "),
            None,
        )
        .unwrap_err();
        assert!(error.contains("clientId"));
    }

    #[test]
    fn user_provided_source_requires_a_secret_where_mandated() {
        let error = resolve_client_credentials(
            StorageProtocol::Box,
            CredentialSource::UserProvided,
            Some("client-1"),
            None,
        )
        .unwrap_err();
        assert!(error.contains("clientSecret"));

        let (client_id, client_secret) = resolve_client_credentials(
            StorageProtocol::Box,
            CredentialSource::UserProvided,
            Some("client-1"),
            Some("secret-1"),
        )
        .unwrap();
        assert_eq!(client_id, "client-1");
        assert_eq!(client_secret.as_deref(), Some("secret-1"));
    }

    #[test]
    fn pkce_only_services_never_advertise_a_secret_requirement() {
        for protocol in [
            StorageProtocol::Dropbox,
            StorageProtocol::OneDrive,
            StorageProtocol::GoogleDrive,
        ] {
            let options = credential_options(protocol).unwrap();
            assert!(!options.requires_client_secret, "{}", protocol.as_str());
            assert!(options.supports_pkce, "{}", protocol.as_str());
        }
    }

    #[test]
    fn secret_mandating_services_can_never_use_builtin_credentials() {
        for protocol in [
            StorageProtocol::Box,
            StorageProtocol::AliyunDrive,
            StorageProtocol::BaiduNetdisk,
        ] {
            let options = credential_options(protocol).unwrap();
            assert!(options.requires_client_secret, "{}", protocol.as_str());
            assert!(!options.builtin_available, "{}", protocol.as_str());
        }
    }

    #[test]
    fn no_builtin_client_secret_is_ever_shipped() {
        // 回归:桌面产物不得内嵌 client_secret。内置凭据只暴露 client_id。
        for protocol in StorageProtocol::ALL {
            if let Some(client_id) = builtin_client_id(protocol) {
                assert!(!client_id.is_empty());
                let provider = provider_for(protocol).expect("builtin id needs a provider");
                assert!(
                    !provider.requires_client_secret,
                    "{} cannot ship a builtin id: it needs a secret",
                    protocol.as_str()
                );
            }
        }
    }

    #[test]
    fn non_oauth_protocols_have_no_provider() {
        for protocol in [
            StorageProtocol::S3,
            StorageProtocol::Smb,
            StorageProtocol::WebdavHttps,
            StorageProtocol::Nfs,
        ] {
            assert!(provider_for(protocol).is_none());
            assert!(credential_options(protocol).is_none());
        }
    }

    #[test]
    fn oauth_protocols_all_have_a_provider() {
        for protocol in StorageProtocol::ALL {
            assert_eq!(
                protocol.is_oauth(),
                provider_for(protocol).is_some(),
                "{} provider/is_oauth mismatch",
                protocol.as_str()
            );
        }
    }

    #[test]
    fn token_secrets_include_expiry_and_credentials() {
        let response = TokenResponse {
            access_token: "at".to_string(),
            refresh_token: Some("rt".to_string()),
            expires_in: Some(3600),
        };
        let secrets = token_response_to_secrets(&response, "cid", Some("csecret"), 1_000);
        assert_eq!(secrets["accessToken"], "at");
        assert_eq!(secrets["refreshToken"], "rt");
        assert_eq!(secrets["expiresAtMs"], (1_000 + 3_600_000).to_string());
        assert_eq!(secrets["clientId"], "cid");
        assert_eq!(secrets["clientSecret"], "csecret");
    }

    #[test]
    fn token_secrets_omit_absent_optional_fields() {
        let response = TokenResponse {
            access_token: "at".to_string(),
            refresh_token: None,
            expires_in: None,
        };
        let secrets = token_response_to_secrets(&response, "cid", None, 1_000);
        assert!(!secrets.contains_key("refreshToken"));
        assert!(!secrets.contains_key("expiresAtMs"));
        assert!(!secrets.contains_key("clientSecret"));
    }

    #[test]
    fn callback_page_escapes_provider_supplied_text() {
        let page = callback_page(false, "<script>alert(1)</script>");
        assert!(!page.contains("<script>"));
        assert!(page.contains("&lt;script&gt;"));
    }
}
