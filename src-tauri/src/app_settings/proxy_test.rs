//! 代理连通性测试。
//!
//! 从 `app_settings.rs` 整块搬出来,内容一行没改。用户在设置里填完代理后点
//! 「测试」走这条路:拿固定的轻量端点发一次请求,把结果归成几个 `reason`
//! 交给前端做 i18n。
//!
//! 两处安全/正确性上的讲究都在原注释里,搬动时一并带过来了:目标 URL **不由
//! 调用方指定**(否则远程配对设备能把本命令当任意请求的转发器),以及
//! https 经 HTTP 代理时 407 会变成隧道错误而不是 407 响应,只能沿 source 链认。

use super::*;

/// 代理连通性测试目标。用固定的轻量端点,避免调用方指定 URL 把本命令
/// 变成任意请求的转发器(远程配对设备也能触发 RPC)。
pub(super) const PROXY_TEST_URL: &str = "https://www.gstatic.com/generate_204";
pub(super) const PROXY_TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 代理测试结果。用户可见文案由前端按 `reason` 走 i18n,
/// `detail` 仅承载底层错误原文用于排查。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub success: bool,
    /// 稳定的机器可读原因码:ok / empty_url / invalid_url / client_build_failed
    /// / timeout / connect_failed / proxy_auth_required / http_error / request_failed
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

impl ProxyTestResult {
    fn failure(reason: &str, detail: Option<String>) -> Self {
        Self {
            success: false,
            reason: reason.to_string(),
            detail,
            status_code: None,
            latency_ms: None,
        }
    }
}

pub(super) fn build_proxy_test_client(
    settings: &ProxySettings,
) -> Result<reqwest::Client, ProxyTestResult> {
    // 复用保存设置时的归一化规则,测试的目标与实际生效的代理保持一致
    // (例如 "127.0.0.1:7890" 会补全为 "http://127.0.0.1:7890")。
    let proxy_url = normalize_proxy_url(&settings.url);
    if proxy_url.is_empty() {
        return Err(ProxyTestResult::failure("empty_url", None));
    }

    let mut proxy = reqwest::Proxy::all(&proxy_url)
        .map_err(|error| ProxyTestResult::failure("invalid_url", Some(error.to_string())))?;
    let username = settings.username.trim();
    if !username.is_empty() {
        proxy = proxy.basic_auth(username, settings.password.trim());
    }
    // 故意不套用 no_proxy:固定测试目标是远端公网地址,绕过规则只会
    // 让请求不经代理直连,从而把失败的代理误报成可用。

    reqwest::Client::builder()
        .proxy(proxy)
        .timeout(PROXY_TEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| ProxyTestResult::failure("client_build_failed", Some(error.to_string())))
}

/// https 目标经 HTTP 代理时走 CONNECT 隧道,代理返回的 407 会变成隧道建立失败
/// (hyper-util 的 `TunnelError::ProxyAuthRequired`),而不是一个 407 响应。
/// 该错误类型未公开导出,只能沿 source 链匹配文案区分“需要认证”与“连不上”。
pub(super) fn error_chain_needs_proxy_authentication(
    error: &(dyn std::error::Error + 'static),
) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        let message = source.to_string().to_ascii_lowercase();
        if message.contains("proxy authorization required")
            || message.contains("proxy authentication required")
        {
            return true;
        }
        current = source.source();
    }
    false
}

pub(super) async fn run_proxy_connection_test(
    settings: &ProxySettings,
    test_url: &str,
) -> ProxyTestResult {
    let client = match build_proxy_test_client(settings) {
        Ok(client) => client,
        Err(result) => return result,
    };

    let started = std::time::Instant::now();
    let response = match client.get(test_url).send().await {
        Ok(response) => response,
        Err(error) => {
            // 认证判定必须早于 is_connect():隧道认证失败同时也算连接失败。
            let reason = if error_chain_needs_proxy_authentication(&error) {
                "proxy_auth_required"
            } else if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connect_failed"
            } else {
                "request_failed"
            };
            return ProxyTestResult::failure(reason, Some(error.to_string()));
        }
    };

    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let status = response.status();
    // 明文 HTTP 目标不走隧道,代理的 407 是一个正常响应,在这里判定。
    if status == reqwest::StatusCode::PROXY_AUTHENTICATION_REQUIRED {
        return ProxyTestResult {
            success: false,
            reason: "proxy_auth_required".to_string(),
            detail: None,
            status_code: Some(status.as_u16()),
            latency_ms: Some(latency_ms),
        };
    }
    // 重定向已禁用,3xx 说明请求已穿过代理到达目标,同样算连通。
    let success = status.is_success() || status.is_redirection();
    ProxyTestResult {
        success,
        reason: if success { "ok" } else { "http_error" }.to_string(),
        detail: None,
        status_code: Some(status.as_u16()),
        latency_ms: Some(latency_ms),
    }
}

#[tauri::command]
pub async fn test_proxy_connection(
    proxy_settings: ProxySettings,
) -> Result<ProxyTestResult, String> {
    Ok(run_proxy_connection_test(&proxy_settings, PROXY_TEST_URL).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn proxy_test_rejects_empty_and_invalid_proxy_urls() {
        let empty = test_proxy_connection(ProxySettings::default())
            .await
            .unwrap();
        assert!(!empty.success);
        assert_eq!(empty.reason, "empty_url");
        assert_eq!(empty.status_code, None);

        let invalid = test_proxy_connection(ProxySettings {
            url: "http://".to_string(),
            ..ProxySettings::default()
        })
        .await
        .unwrap();
        assert!(!invalid.success);
        assert_eq!(invalid.reason, "invalid_url");
    }

    /// 明文 http 目标经代理时用绝对形式请求行,代理的响应即最终响应,
    /// 因此可以用一个假代理确定性地覆盖 407 与连通两条分支。
    const PROXY_TEST_HTTP_TARGET: &str = "http://proxy-test.invalid/generate_204";

    fn spawn_fake_proxy(
        responses: Vec<&'static [u8]>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
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
                requests.push(String::from_utf8_lossy(&request).to_string());
                stream.write_all(response).unwrap();
                let _ = stream.flush();
            }
            requests
        });
        (proxy_url, handle)
    }

    #[tokio::test]
    async fn proxy_test_reports_proxy_auth_required_and_success_via_proxy() {
        let (proxy_url, server) = spawn_fake_proxy(vec![
            b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);

        let auth_required = run_proxy_connection_test(
            &ProxySettings {
                url: proxy_url.clone(),
                username: "user".to_string(),
                password: "secret".to_string(),
                ..ProxySettings::default()
            },
            PROXY_TEST_HTTP_TARGET,
        )
        .await;
        assert!(!auth_required.success);
        assert_eq!(auth_required.reason, "proxy_auth_required");
        assert_eq!(auth_required.status_code, Some(407));

        let connected = run_proxy_connection_test(
            &ProxySettings {
                url: proxy_url,
                ..ProxySettings::default()
            },
            PROXY_TEST_HTTP_TARGET,
        )
        .await;
        assert!(connected.success);
        assert_eq!(connected.reason, "ok");
        assert_eq!(connected.status_code, Some(204));
        assert!(connected.latency_ms.is_some());

        let requests = server.join().unwrap();
        // 凭据必须发给代理,且请求确实经过了代理(绝对形式请求行)。
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("proxy-authorization: basic"));
        assert!(requests[0].starts_with(&format!("GET {PROXY_TEST_HTTP_TARGET}")));
        assert!(!requests[1]
            .to_ascii_lowercase()
            .contains("proxy-authorization:"));
    }

    #[tokio::test]
    async fn proxy_test_treats_target_errors_as_failure() {
        let (proxy_url, server) = spawn_fake_proxy(vec![
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);

        let result = run_proxy_connection_test(
            &ProxySettings {
                url: proxy_url,
                ..ProxySettings::default()
            },
            PROXY_TEST_HTTP_TARGET,
        )
        .await;

        assert!(!result.success);
        assert_eq!(result.reason, "http_error");
        assert_eq!(result.status_code, Some(502));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn proxy_test_ignores_no_proxy_so_dead_proxies_are_not_reported_healthy() {
        // no_proxy 覆盖测试目标时若被套用,请求会绕过代理直连,
        // 已关闭的代理会被误判为可用。这里的目标服务器是活的、代理是死的:
        // 只要结果不是 ok,就说明请求没有绕过代理。
        let (target_url, target) = spawn_fake_proxy(vec![
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ]);
        let target_host = target_url.trim_start_matches("http://").to_string();

        let dead_listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let dead_proxy = format!("http://{}", dead_listener.local_addr().unwrap());
        drop(dead_listener);

        let result = run_proxy_connection_test(
            &ProxySettings {
                url: dead_proxy,
                no_proxy: format!("127.0.0.1,{target_host}"),
                ..ProxySettings::default()
            },
            &target_url,
        )
        .await;

        assert!(!result.success);
        assert_ne!(result.reason, "ok");
        drop(target);
    }

    #[test]
    fn classifies_connect_tunnel_auth_failure_as_proxy_auth_required() {
        // https 目标的 407 来自 CONNECT 隧道失败,错误类型未公开导出,
        // 这里用等价的 source 链锁定文案匹配逻辑。
        #[derive(Debug)]
        struct Inner;
        impl std::fmt::Display for Inner {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("tunnel error: proxy authorization required")
            }
        }
        impl std::error::Error for Inner {}

        #[derive(Debug)]
        struct Outer(Inner);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("error trying to connect")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        #[derive(Debug)]
        struct Refused;
        impl std::fmt::Display for Refused {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("tcp connect error: connection refused")
            }
        }
        impl std::error::Error for Refused {}

        assert!(error_chain_needs_proxy_authentication(&Outer(Inner)));
        assert!(!error_chain_needs_proxy_authentication(&Refused));
    }
}
