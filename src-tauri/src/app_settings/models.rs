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

/// 解析 base URL 的全部地址,**不过滤私网**,用于本机探测的地址故障转移。
///
/// 与 [`resolve_remote_model_addresses`] 的过滤是刻意的差别:那个函数服务于
/// [`ModelDetectionPolicy::PairedDevice`],过滤是防手机端拿桌面端当跳板探内网的
/// SSRF 闸门;本机用户填 `http://127.0.0.1:11434/v1`(Ollama)这类地址是正常用法,
/// 在这里过滤会把能用的配置判死。
///
/// 解析失败返回空 vec 而非报错:故障转移是尽力而为的优化,拿不到地址就退回
/// 让 reqwest 自己解析的普通路径,不能因此让探测直接失败。
pub(super) async fn resolve_local_model_addresses(base_url: &url::Url) -> Vec<SocketAddr> {
    let Some(host) = base_url.host_str() else {
        return Vec::new();
    };
    // IP 字面量 host 无需故障转移:只有一个地址,换来换去还是它。
    if host.parse::<IpAddr>().is_ok() {
        return Vec::new();
    }
    let Some(port) = base_url.port_or_known_default() else {
        return Vec::new();
    };
    match tokio::net::lookup_host((host, port)).await {
        Ok(addresses) => addresses.collect(),
        Err(_) => Vec::new(),
    }
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

/// 已知的「Anthropic / Coding Plan 兼容子路径」后缀,按长度降序排列以保证最长后缀优先命中
/// (否则 `/anthropic` 会把 `/api/anthropic` 提前截断,剥出残缺的 `.../api` 根)。
///
/// 这类 base URL 只承载对话协议,模型目录通常挂在剥离后缀的 API 根上:
/// `https://token-plan-cn.xiaomimimo.com/anthropic` 的模型列表在
/// `https://token-plan-cn.xiaomimimo.com/v1/models`,拼成 `/anthropic/v1/models` 会 404。
const KNOWN_COMPAT_SUFFIXES: &[&str] = &[
    "/api/claudecode",
    "/api/anthropic",
    "/apps/anthropic",
    "/api/coding",
    "/claudecode",
    "/anthropic",
    "/step_plan",
    "/coding",
    "/claude",
];

/// 单次探测请求超时。候选端点会串行尝试,单请求不能占满整体预算。
const MODEL_DETECT_REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// 全部候选探测的总时间预算。超出后停止继续尝试并回报已知错误,
/// 避免「检测模型」按钮在慢上游上长时间无响应。
///
/// 地址故障转移会把整条候选链重跑多次,因此这个预算由调用方换算成一个
/// **共享 deadline**(见 [`fetch_agent_model_json_from_candidates`] 的 `deadline` 形参),
/// 多次尝试共用同一上限,按钮最坏等待不随重试次数线性放大。
pub(super) const MODEL_DETECT_TOTAL_BUDGET: Duration = Duration::from_secs(30);

/// 建连超时(含 TLS 握手)。
///
/// 存在这样的上游:某个 A 记录接受 TCP 却让 TLS 握手永不返回(实测
/// `api.deepseek.com` 的 5 个地址里有 1 个如此)。hyper 的 happy-eyeballs 只在
/// **TCP connect 失败**时换下一个地址 —— TCP 已经成功,它就锁定这条死连接,
/// 于是整个请求耗到 [`MODEL_DETECT_REQUEST_TIMEOUT`] 才失败。
///
/// reqwest 的 `connect_timeout` 包住的是整个 connector(TCP + TLS),所以这个值
/// 能把黑洞地址在 4s 内判死,剩下的预算留给真正可用的地址。
pub(super) const MODEL_DETECT_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

/// 错误详情中保留的响应体长度上限,避免把整页 HTML 错误页塞进错误串。
const ERROR_BODY_MAX_CHARS: usize = 200;

/// 模型探测使用的 CLI User-Agent。
///
/// 不少聚合网关(如 agentrouter.org)按 User-Agent 名称前缀做客户端白名单:
/// 缺省 UA 或通用 UA 会先被 401 `unauthorized client detected` 拦掉,根本走不到鉴权。
/// 白名单只看名称前缀、不校验版本号,所以用静态值即可,不会随 CLI 升级失效。
const CLAUDE_CLI_DETECT_USER_AGENT: &str = "claude-cli/2.1.161 (external, cli)";
const CODEX_CLI_DETECT_USER_AGENT: &str = "codex_cli_rs/0.77.0";

/// 探测(含余额查询)客户端的默认 User-Agent。
pub(super) fn default_model_detect_user_agent() -> &'static str {
    CLAUDE_CLI_DETECT_USER_AGENT
}

/// base URL 是否以 OpenAI 风格版本段 `/v{N}` 结尾(`/v1`、智谱 `.../paas/v4`)。
/// 这类 URL 版本号已在路径里,模型端点是 `{base}/models`,再补 `/v1` 会 404。
fn ends_with_version_segment(url: &str) -> bool {
    url.rsplit('/')
        .next()
        .unwrap_or("")
        .strip_prefix('v')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// 若 base URL 以任一 [`KNOWN_COMPAT_SUFFIXES`] 结尾,返回剥离后的剩余部分。
fn strip_compat_suffix(base_url: &str) -> Option<&str> {
    KNOWN_COMPAT_SUFFIXES
        .iter()
        .find(|suffix| base_url.ends_with(**suffix))
        .map(|suffix| &base_url[..base_url.len() - suffix.len()])
}

/// 剥离尾部版本段 `/v{N}`,用于推导 API 根。
fn strip_version_segment(base_url: &str) -> Option<&str> {
    ends_with_version_segment(base_url)
        .then(|| base_url.rfind('/').map(|index| &base_url[..index]))
        .flatten()
}

/// 剥离后的串是否仍是「scheme://host」及更深路径(防止把 host 也吃掉)。
fn keeps_origin(candidate: &str) -> bool {
    candidate
        .find("://")
        .is_some_and(|index| candidate.len() > index + 3)
}

fn base_without_query(base_url: &url::Url) -> String {
    let mut url = base_url.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.to_string().trim_end_matches('/').to_string()
}

fn push_unique(out: &mut Vec<String>, candidate: String) {
    if !out.iter().any(|existing| existing == &candidate) {
        out.push(candidate);
    }
}

/// 构造模型列表端点候选,按「最可能正确」排序:
///
/// 1. base 以版本段结尾 → `{base}/models`;版本段非 `/v1` 时再兜底 `{base}/v1/models`
/// 2. 否则 → `{base}/v1/models`
/// 3. base 命中兼容子路径 → 剥离后追加 `{root}/v1/models`、`{root}/models`
///
/// 所有候选与 base URL 同源,只改 path,因此 [`ModelDetectionPolicy::PairedDevice`]
/// 的 DNS 固定(`resolve_to_addrs`)对候选同样生效。
pub(super) fn model_endpoint_candidates(base_url: &url::Url) -> Vec<String> {
    let root = base_without_query(base_url);
    let mut candidates = Vec::new();

    if ends_with_version_segment(&root) {
        push_unique(&mut candidates, format!("{root}/models"));
        if !root.ends_with("/v1") {
            push_unique(&mut candidates, format!("{root}/v1/models"));
        }
    } else {
        push_unique(&mut candidates, format!("{root}/v1/models"));
    }

    if let Some(stripped) = strip_compat_suffix(&root) {
        let stripped = stripped.trim_end_matches('/');
        if keeps_origin(stripped) {
            push_unique(&mut candidates, format!("{stripped}/v1/models"));
            push_unique(&mut candidates, format!("{stripped}/models"));
        }
    }

    candidates
}

/// New API / One API 系网关的公开模型目录(`/api/pricing`)候选。
///
/// 仅在全部带鉴权的 `/models` 候选都「端点不存在」时作为兜底 —— 一旦出现过鉴权失败,
/// 就不能用公开目录顶替,否则会把「API Key 无效」掩盖成一份查不到权限的模型列表。
pub(super) fn public_model_catalog_candidates(base_url: &url::Url) -> Vec<String> {
    let root = base_without_query(base_url);
    let api_root = strip_compat_suffix(&root)
        .or_else(|| strip_version_segment(&root))
        .map(|stripped| stripped.trim_end_matches('/'))
        .filter(|stripped| keeps_origin(stripped))
        .unwrap_or(root.as_str());

    let mut candidates = vec![format!("{api_root}/api/pricing")];
    // 再补一条「站点根」候选。用 url 改 path 而非拼字符串,IPv6 字面量 host 才能保留方括号。
    let mut origin = base_url.clone();
    origin.set_query(None);
    origin.set_fragment(None);
    origin.set_path("/api/pricing");
    push_unique(&mut candidates, origin.to_string());
    candidates
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
        AgentSetupKind::Dsh => [
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

/// 单个端点的探测结果,决定是否继续尝试下一个候选。
#[derive(Debug)]
enum EndpointOutcome {
    /// 成功拿到 JSON。
    Found(serde_json::Value),
    /// 端点不存在(404/405)或响应不是模型目录 —— 换下一个候选。
    Missing(String),
    /// 上游给了响应但拒绝(鉴权失败、Cloudflare 拦截等)—— 换端点不会好转,
    /// 但仍记录以便所有候选耗尽后回报。
    Failed(String),
    /// 请求根本没拿到响应(DNS / TLS / 建连 / 超时)。
    ///
    /// 必须与 [`EndpointOutcome::Failed`] 分开:这类失败既不能推断「API Key 无效」
    /// (否则会掩盖成鉴权问题、并压掉公开目录兜底),也是唯一值得换 IP 地址重试的情形。
    Transport(String),
}

/// 一轮探测里出现过哪几类失败,供调用方分别决策。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DetectionFailures {
    /// 出现过「上游拒绝」(401/403/Cloudflare 等)。出现过就不能用公开目录兜底,
    /// 否则会把「API Key 无效」显示成一份查不到权限的模型列表。
    pub auth: bool,
    /// 出现过传输层失败。换一个上游地址有可能好转。
    pub transport: bool,
}

/// User-Agent 尝试顺序:先按 agent 类型用最贴近真实 CLI 的 UA(网关白名单通常按名称
/// 前缀匹配),再退回另一种 CLI UA。两个都试完仍失败才认为不是 UA 问题。
fn model_user_agent_attempts(kind: &AgentSetupKind) -> [&'static str; 2] {
    match kind {
        AgentSetupKind::Codex | AgentSetupKind::Dsh => {
            [CODEX_CLI_DETECT_USER_AGENT, CLAUDE_CLI_DETECT_USER_AGENT]
        }
        AgentSetupKind::ClaudeCode => [CLAUDE_CLI_DETECT_USER_AGENT, CODEX_CLI_DETECT_USER_AGENT],
    }
}

/// 网关的「客户端未授权」拦截:UA 不在白名单时先于鉴权返回 401/403。
/// 命中即说明值得换 UA 重试,而不是判定 API Key 无效。
fn looks_like_client_rejection(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    body.contains("unauthorized client")
        || body.contains("unauthorized_client")
        || body.contains("client not allowed")
        || body.contains("forbidden client")
        || body.contains("invalid client")
}

fn looks_like_cloudflare_challenge(headers: &reqwest::header::HeaderMap, body: &str) -> bool {
    let challenged = headers
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"));
    if challenged {
        return true;
    }

    let body = body.to_ascii_lowercase();
    body.contains("just a moment")
        && (body.contains("cloudflare")
            || body.contains("cf-chl")
            || body.contains("challenge-platform"))
}

fn describe_cloudflare_challenge(status: reqwest::StatusCode) -> String {
    format!(
        "HTTP {status}: Cloudflare challenge blocked the API request. Exempt the model API path from browser challenges or allow non-browser API clients."
    )
}

fn truncate_body(body: &str) -> String {
    let body = body.trim();
    if body.chars().count() <= ERROR_BODY_MAX_CHARS {
        return body.to_string();
    }
    let mut out: String = body.chars().take(ERROR_BODY_MAX_CHARS).collect();
    out.push('…');
    out
}

/// 把传输层错误连同 source 链上的真因拼成一句话。
///
/// `reqwest::Error` 的 `Display` 只写 `"error sending request for url (...)"`,**从不打印
/// source** —— 于是超时、DNS 失败、TLS 失败在 UI 上长得一模一样,用户只能看到一句
/// 无从下手的「发送请求失败」。这里沿 source 链取最深一层描述补上去。
///
/// 另外附一句可操作提示:传输层失败往往是上游某个 IP 无响应,配置代理即可绕开。
fn describe_transport_error(error: &reqwest::Error) -> String {
    let mut detail = error.to_string();
    let mut deepest: Option<String> = None;
    let mut current = std::error::Error::source(error);
    while let Some(source) = current {
        deepest = Some(source.to_string());
        current = source.source();
    }
    // 最深一层通常是 `operation timed out` / `dns error` / `connection refused`;
    // 它已被外层文案包含时(某些 source 只是复述)不重复追加。
    if let Some(cause) = deepest.filter(|cause| {
        let cause = cause.trim();
        !cause.is_empty() && !detail.contains(cause)
    }) {
        detail = format!("{detail}: {cause}");
    }
    if error.is_timeout() || error.is_connect() {
        detail.push_str(
            " (upstream address did not respond; configure a proxy in Settings and retry)",
        );
    }
    detail
}

fn describe_http_error(status: reqwest::StatusCode, body: &str) -> String {
    let body = truncate_body(body);
    if body.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body}")
    }
}

/// 响应是否确实是模型目录。有些网关对未知路径返回 200 + HTML 首页或
/// `{"success":false}`,不校验就会把「探测成功但零模型」当作结果。
fn contains_model_catalog(value: &serde_json::Value) -> bool {
    !parse_model_ids(value.clone()).is_empty()
}

async fn probe_model_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    kind: &AgentSetupKind,
    api_key: &str,
) -> EndpointOutcome {
    let auth_attempts = model_auth_attempts(kind);
    let user_agents = model_user_agent_attempts(kind);
    let mut last_failure: Option<String> = None;

    for (auth_index, auth) in auth_attempts.into_iter().enumerate() {
        for (ua_index, user_agent) in user_agents.into_iter().enumerate() {
            let request = apply_model_auth(client.get(endpoint), api_key, auth)
                .header(reqwest::header::USER_AGENT, user_agent)
                .header(reqwest::header::ACCEPT, "application/json")
                .timeout(MODEL_DETECT_REQUEST_TIMEOUT);
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    // 传输层错误(DNS/TLS/超时)与鉴权头和 UA 无关,换组合没有意义。
                    return EndpointOutcome::Transport(describe_transport_error(&error));
                }
            };

            let status = response.status();
            let response_headers = response.headers().clone();
            if status.is_success() {
                let body = response.text().await.unwrap_or_default();
                // 2xx 说明鉴权头与 UA 都已被接受,换组合重试没有意义:
                // 响应不是模型目录时直接判定该候选路径不对,交给下一个候选。
                return match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(value) if contains_model_catalog(&value) => EndpointOutcome::Found(value),
                    Ok(_) => EndpointOutcome::Missing(format!(
                        "HTTP {status}: response has no model list"
                    )),
                    Err(error) => {
                        EndpointOutcome::Missing(format!("HTTP {status}: invalid JSON ({error})"))
                    }
                };
            }

            let body = response.text().await.unwrap_or_default();
            if looks_like_cloudflare_challenge(&response_headers, &body) {
                return EndpointOutcome::Failed(describe_cloudflare_challenge(status));
            }
            if matches!(
                status,
                reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
            ) {
                // 端点不存在与鉴权头/UA 无关,直接判定该候选不可用。
                return EndpointOutcome::Missing(describe_http_error(status, &body));
            }

            let detail = describe_http_error(status, &body);
            let auth_rejected = matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            );
            // 命中「客户端未授权」时优先换 UA;否则 UA 相同无需重复尝试,直接换鉴权头。
            let retry_user_agent = auth_rejected
                && looks_like_client_rejection(&detail)
                && ua_index + 1 < user_agents.len();
            last_failure = Some(detail);
            if retry_user_agent {
                continue;
            }
            if auth_rejected && auth_index + 1 < auth_attempts.len() {
                break;
            }
            return EndpointOutcome::Failed(last_failure.take().unwrap_or_default());
        }
    }

    // 走到这里说明鉴权头/UA 组合全部耗尽,最后一次失败详情最能反映真实原因。
    EndpointOutcome::Failed(last_failure.unwrap_or_else(|| "Model detection failed".to_string()))
}

/// 按候选顺序探测模型目录端点。
///
/// 返回 `Ok((json, endpoint))`;`failures` 记录出现过哪几类失败,供调用方分别决定
/// 「能否用公开目录兜底」(见 [`DetectionFailures::auth`])与「是否值得换上游地址重试」
/// (见 [`DetectionFailures::transport`])。
///
/// `deadline` 由调用方给出而非在此自行计算:地址故障转移会把整条候选链重跑多次,
/// 共用一个 deadline 才能保证总等待不随重试次数放大。
pub(super) async fn fetch_agent_model_json_from_candidates(
    client: &reqwest::Client,
    candidates: &[String],
    kind: &AgentSetupKind,
    api_key: &str,
    failures: &mut DetectionFailures,
    deadline: std::time::Instant,
) -> Result<(serde_json::Value, String), String> {
    let mut errors: Vec<String> = Vec::new();

    for endpoint in candidates {
        if std::time::Instant::now() >= deadline {
            errors.push("timed out before all endpoints were tried".to_string());
            break;
        }
        match probe_model_endpoint(client, endpoint, kind, api_key).await {
            EndpointOutcome::Found(value) => return Ok((value, endpoint.clone())),
            EndpointOutcome::Missing(detail) => {
                errors.push(format!("{endpoint} -> {detail}"));
            }
            EndpointOutcome::Failed(detail) => {
                failures.auth = true;
                errors.push(format!("{endpoint} -> {detail}"));
            }
            EndpointOutcome::Transport(detail) => {
                failures.transport = true;
                errors.push(format!("{endpoint} -> {detail}"));
            }
        }
    }

    Err(format!(
        "Model detection failed: {}",
        if errors.is_empty() {
            "no endpoint candidates".to_string()
        } else {
            errors.join("; ")
        }
    ))
}

/// 从 New API 系公开目录(`/api/pricing`)读取模型名。
///
/// 该端点无需鉴权,字段是 `model_name` 而非 `id`,因此单独解析。
pub(super) async fn fetch_public_model_catalog(
    client: &reqwest::Client,
    candidates: &[String],
    kind: &AgentSetupKind,
) -> Option<Vec<String>> {
    let user_agent = model_user_agent_attempts(kind)[0];
    for endpoint in candidates {
        let response = client
            .get(endpoint)
            .header(reqwest::header::USER_AGENT, user_agent)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(MODEL_DETECT_REQUEST_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            continue;
        }
        let Ok(value) = response.json::<serde_json::Value>().await else {
            continue;
        };
        let models = parse_public_catalog_model_ids(&value);
        if !models.is_empty() {
            return Some(models);
        }
    }
    None
}

/// 解析 `/api/pricing` 形态的公开目录:`{"data":[{"model_name":"..."}]}`。
pub(super) fn parse_public_catalog_model_ids(value: &serde_json::Value) -> Vec<String> {
    let items = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array());
    let Some(items) = items else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in items {
        if let Some(name) = item
            .get("model_name")
            .or_else(|| item.get("model"))
            .and_then(serde_json::Value::as_str)
        {
            push_model_id(&mut out, name);
        }
    }
    out.sort_by_key(|model| model.to_ascii_lowercase());
    out.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    out
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
        // 余额端点与模型端点常共用同一 UA 白名单,缺省 UA 会被 401 拦掉。
        .header(
            reqwest::header::USER_AGENT,
            default_model_detect_user_agent(),
        )
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

    /// 极小的测试 HTTP 服务:按到达顺序回放 `responses`,并记录每次请求头,
    /// 用于断言候选顺序、UA 与鉴权头重试行为。
    fn serve_responses(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => request.extend_from_slice(&chunk[..count]),
                    }
                }
                seen.push(String::from_utf8_lossy(&request).to_string());
                let reason = if status == 200 { "OK" } else { "Error" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                let _ = stream.write_all(body.as_bytes());
            }
            seen
        });
        (origin, handle)
    }

    fn request_line(request: &str) -> String {
        request.lines().next().unwrap_or("").to_string()
    }

    fn header_value(request: &str, name: &str) -> String {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        request
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
            .map(|line| line[prefix.len()..].trim().to_string())
            .unwrap_or_default()
    }

    /// 测试用的共享 deadline:给足预算,让测试只考察探测逻辑而非超时。
    fn test_deadline() -> std::time::Instant {
        std::time::Instant::now() + MODEL_DETECT_TOTAL_BUDGET
    }

    #[tokio::test]
    async fn retries_unauthorized_model_requests_with_api_key_headers() {
        // 首个 UA 组合返回 401(且不是「客户端未授权」),应改换鉴权头而不是换 UA。
        let (origin, server) = serve_responses(vec![
            (401, "{\"error\":\"invalid auth\"}"),
            (200, r#"{"data":[{"id":"fallback-model"}]}"#),
        ]);
        let endpoint = format!("{origin}/v1/models");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let (value, used) = fetch_agent_model_json_from_candidates(
            &client,
            std::slice::from_ref(&endpoint),
            &AgentSetupKind::Codex,
            "sk-test",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap();

        assert_eq!(parse_model_ids(value), vec!["fallback-model"]);
        assert_eq!(used, endpoint);
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(header_value(&seen[0], "authorization"), "Bearer sk-test");
        assert!(header_value(&seen[0], "x-api-key").is_empty());
        assert_eq!(header_value(&seen[1], "x-api-key"), "sk-test");
        assert_eq!(header_value(&seen[1], "anthropic-version"), "2023-06-01");
    }

    #[tokio::test]
    async fn sends_cli_user_agent_and_retries_client_rejections_with_another_agent() {
        // agentrouter.org 形态:UA 不在白名单时先于鉴权返回 401 unauthorized client。
        let (origin, server) = serve_responses(vec![
            (
                401,
                r#"{"error":{"message":"unauthorized client detected"}}"#,
            ),
            (200, r#"{"data":[{"id":"claude-opus-5"}]}"#),
        ]);
        let endpoint = format!("{origin}/v1/models");

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let (value, _) = fetch_agent_model_json_from_candidates(
            &client,
            &[endpoint],
            &AgentSetupKind::Codex,
            "sk-test",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap();

        assert_eq!(parse_model_ids(value), vec!["claude-opus-5"]);
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 2);
        // Codex 先用 codex_cli_rs,被拒后换 claude-cli;两次都必须带 UA。
        assert_eq!(
            header_value(&seen[0], "user-agent"),
            CODEX_CLI_DETECT_USER_AGENT
        );
        assert_eq!(
            header_value(&seen[1], "user-agent"),
            CLAUDE_CLI_DETECT_USER_AGENT
        );
        // 换 UA 重试必须沿用同一鉴权头,不能顺带换掉。
        assert_eq!(header_value(&seen[1], "authorization"), "Bearer sk-test");
    }

    #[tokio::test]
    async fn falls_back_to_next_candidate_when_compat_path_returns_404() {
        // token-plan-cn.xiaomimimo.com/anthropic 形态:/anthropic/v1/models 404,
        // 剥离 /anthropic 后的 /v1/models 才是真正的模型目录。
        let (origin, server) = serve_responses(vec![
            (404, "<html>404 Not Found</html>"),
            (200, r#"{"data":[{"id":"MiMo-VL"}]}"#),
        ]);
        let base = validate_model_base_url(
            &format!("{origin}/anthropic"),
            ModelDetectionPolicy::LocalUser,
        )
        .unwrap();
        let candidates = model_endpoint_candidates(&base);
        assert_eq!(
            candidates,
            vec![
                format!("{origin}/anthropic/v1/models"),
                format!("{origin}/v1/models"),
                format!("{origin}/models"),
            ]
        );

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let (value, used) = fetch_agent_model_json_from_candidates(
            &client,
            &candidates,
            &AgentSetupKind::ClaudeCode,
            "sk-test",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap();

        assert_eq!(parse_model_ids(value), vec!["MiMo-VL"]);
        assert_eq!(used, format!("{origin}/v1/models"));
        assert_eq!(failures, DetectionFailures::default());
        let seen = server.join().unwrap();
        // 404 不该触发 UA / 鉴权头重试,应立即换下一个候选。
        assert_eq!(seen.len(), 2);
        assert!(request_line(&seen[0]).contains("/anthropic/v1/models"));
        assert!(request_line(&seen[1]).contains("/v1/models"));
    }

    #[tokio::test]
    async fn treats_non_catalog_success_as_missing_endpoint() {
        // 有些网关对未知路径返回 200 + 非目录 JSON,不能当成「零模型」成功。
        let (origin, server) = serve_responses(vec![
            (200, r#"{"success":false,"message":"not found"}"#),
            (200, r#"{"data":[{"id":"real-model"}]}"#),
        ]);
        let candidates = vec![
            format!("{origin}/anthropic/v1/models"),
            format!("{origin}/v1/models"),
        ];

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let (value, used) = fetch_agent_model_json_from_candidates(
            &client,
            &candidates,
            &AgentSetupKind::ClaudeCode,
            "sk-test",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap();

        assert_eq!(parse_model_ids(value), vec!["real-model"]);
        assert_eq!(used, candidates[1]);
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn reports_auth_failure_instead_of_silently_succeeding() {
        // 鉴权失败时必须把错误报给用户,不能退到公开目录伪装成成功。
        let (origin, server) = serve_responses(vec![
            (401, r#"{"error":{"message":"无效的令牌"}}"#),
            (401, r#"{"error":{"message":"无效的令牌"}}"#),
            (401, r#"{"error":{"message":"无效的令牌"}}"#),
        ]);

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let error = fetch_agent_model_json_from_candidates(
            &client,
            &[format!("{origin}/v1/models")],
            &AgentSetupKind::ClaudeCode,
            "sk-bad",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap_err();

        assert!(failures.auth);
        // 上游给了响应,不是传输层失败 —— 不能触发地址故障转移。
        assert!(!failures.transport);
        assert!(error.contains("HTTP 401"), "unexpected error: {error}");
        assert!(error.contains("无效的令牌"), "unexpected error: {error}");
        drop(server);
    }

    /// 传输层失败不能被记成鉴权失败。
    ///
    /// 记错的后果不只是文案难看:调用方靠 `failures.auth` 决定「要不要报错而不兜底」,
    /// 靠 `failures.transport` 决定「要不要换上游地址重试」。把网络不通算成鉴权失败,
    /// 会同时压掉兜底、又跳过故障转移,还把原因显示成「API Key 无效」。
    #[tokio::test]
    async fn transport_failure_is_not_reported_as_auth_failure() {
        // 绑完立刻释放,拿到一个确定没人监听的端口 —— 连接会被立即拒绝。
        let dead_port = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.local_addr().unwrap().port()
        };

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let error = fetch_agent_model_json_from_candidates(
            &client,
            &[format!("http://127.0.0.1:{dead_port}/v1/models")],
            &AgentSetupKind::Dsh,
            "sk-test",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap_err();

        assert!(failures.transport, "unexpected failures: {failures:?}");
        assert!(!failures.auth, "unexpected failures: {failures:?}");
        assert!(
            !error.contains("HTTP "),
            "transport failure must not look like an HTTP status: {error}"
        );
    }

    /// 传输层报错必须带上 source 链里的真因。
    ///
    /// `reqwest::Error` 的 `Display` 只写 `"error sending request for url (...)"`,用户拿到
    /// 这句话完全无从下手 —— 超时、DNS 失败、连接被拒长得一模一样。
    #[tokio::test]
    async fn transport_error_carries_the_underlying_cause() {
        let dead_port = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.local_addr().unwrap().port()
        };

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let error = fetch_agent_model_json_from_candidates(
            &client,
            &[format!("http://127.0.0.1:{dead_port}/v1/models")],
            &AgentSetupKind::Dsh,
            "sk-test",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap_err();

        let lowered = error.to_ascii_lowercase();
        assert!(
            lowered.contains("refused") || lowered.contains("connect"),
            "error should name the real cause, got: {error}"
        );
        // 建连失败要给出可操作提示(配代理),而不是只丢一句失败。
        assert!(
            error.contains("configure a proxy"),
            "connect failure should hint at the proxy setting: {error}"
        );
    }

    /// 绑一个端口再立刻释放,得到一个确定没人监听的端口。
    fn dead_port() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// 首个地址是黑洞时,探测应换到下一个地址而不是直接失败。
    ///
    /// 这是本次修复的核心:`api.deepseek.com` 的 5 个 A 记录里有 1 个接受 TCP 却让 TLS
    /// 永不返回,而 hyper 的 happy-eyeballs 只在 **TCP connect 失败**时换地址 —— TCP 已经
    /// 成功,它就锁死在那条连接上。换地址只能由我们自己驱动。
    ///
    /// 测试里用「连接被拒」代替「TLS 卡死」当坏地址:两者都是传输层失败、走同一条分支,
    /// 但前者立即返回,不必让测试真的等满建连超时。
    #[tokio::test]
    async fn retries_the_next_address_when_the_first_one_is_a_black_hole() {
        let (origin, server) = serve_responses(vec![(200, r#"{"data":[{"id":"deepseek-v4"}]}"#)]);
        let live_port = origin.rsplit(':').next().unwrap().parse::<u16>().unwrap();
        let black_hole = SocketAddr::from(([127, 0, 0, 1], dead_port()));
        let live = SocketAddr::from(([127, 0, 0, 1], live_port));

        // base URL 刻意不带端口:hyper-util 只在 URL 显式给了端口时才用 URL 的端口覆盖
        // `resolve_to_addrs` 里的端口,不带端口才能让钉住的端口生效。
        let base_url = validate_model_base_url(
            "http://detect-failover.test/v1",
            ModelDetectionPolicy::LocalUser,
        )
        .unwrap();

        let detected = detect_models_over_http(
            &AgentSetupKind::Dsh,
            &base_url,
            "sk-test",
            ModelDetectionPolicy::LocalUser,
            &ProxySettings::default(),
            std::slice::from_ref(&black_hole),
            &[black_hole, live],
        )
        .await
        .unwrap();

        assert_eq!(detected.models, vec!["deepseek-v4"]);
        assert_eq!(server.join().unwrap().len(), 1);
    }

    /// 配了代理时,loopback / 私网 base URL 仍须直连。
    ///
    /// 代理一般不会把请求转回发起方的 loopback,所以本机 base URL(Ollama、应用自带的
    /// local_router)一旦被推去代理就会从「能用」变成「连不上」。用户没有理由为了让模型
    /// 探测工作而手写这些例外。
    #[tokio::test]
    async fn keeps_loopback_base_urls_direct_even_when_a_proxy_is_configured() {
        let (origin, server) = serve_responses(vec![(200, r#"{"data":[{"id":"local-model"}]}"#)]);
        // 指向一个没人监听的端口:一旦请求真被推去代理,这个测试就会失败。
        let proxy_settings = ProxySettings {
            url: format!("http://127.0.0.1:{}", dead_port()),
            ..ProxySettings::default()
        };
        let base_url =
            validate_model_base_url(&format!("{origin}/v1"), ModelDetectionPolicy::LocalUser)
                .unwrap();

        let detected = detect_models_over_http(
            &AgentSetupKind::Dsh,
            &base_url,
            "sk-test",
            ModelDetectionPolicy::LocalUser,
            &proxy_settings,
            &[],
            &[],
        )
        .await
        .unwrap();

        assert_eq!(detected.models, vec!["local-model"]);
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn detect_no_proxy_rules_add_local_networks_without_dropping_user_rules() {
        let rules = detect_no_proxy_rules("example.com, 203.0.113.0/24");
        assert!(rules.starts_with("example.com,203.0.113.0/24,"));
        for expected in ["127.0.0.1", "::1", "localhost", "10.0.0.0/8"] {
            assert!(rules.contains(expected), "missing {expected} in {rules}");
        }
        // 用户已经写了的规则不重复追加。
        let deduped = detect_no_proxy_rules("localhost");
        assert_eq!(deduped.matches("localhost").count(), 1);
    }

    #[tokio::test]
    async fn reports_cloudflare_challenge_without_echoing_html_or_retrying() {
        let challenge_html = r#"<!DOCTYPE html><html><head><title>Just a moment...</title></head><body><script src="/cdn-cgi/challenge-platform/scripts/jsd/main.js"></script></body></html>"#;
        let (origin, server) = serve_responses(vec![(403, challenge_html)]);

        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut failures = DetectionFailures::default();
        let error = fetch_agent_model_json_from_candidates(
            &client,
            &[format!("{origin}/v1/models")],
            &AgentSetupKind::Codex,
            "sk-test",
            &mut failures,
            test_deadline(),
        )
        .await
        .unwrap_err();

        assert!(failures.auth);
        assert!(!failures.transport);
        assert!(error.contains("Cloudflare challenge blocked the API request"));
        assert!(error.contains("allow non-browser API clients"));
        assert!(!error.contains("<!DOCTYPE"));
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn builds_model_endpoint_candidates_for_common_gateway_shapes() {
        let candidates = |raw: &str| {
            let url = validate_model_base_url(raw, ModelDetectionPolicy::LocalUser).unwrap();
            model_endpoint_candidates(&url)
        };

        assert_eq!(
            candidates("https://api.siliconflow.cn"),
            vec!["https://api.siliconflow.cn/v1/models"]
        );
        assert_eq!(
            candidates("https://agentrouter.org"),
            vec!["https://agentrouter.org/v1/models"]
        );
        assert_eq!(
            candidates("https://token-plan-cn.xiaomimimo.com/anthropic"),
            vec![
                "https://token-plan-cn.xiaomimimo.com/anthropic/v1/models",
                "https://token-plan-cn.xiaomimimo.com/v1/models",
                "https://token-plan-cn.xiaomimimo.com/models",
            ]
        );
        // 版本段已在路径里:{base}/models 必须排在 {base}/v1/models 之前。
        assert_eq!(
            candidates("https://open.bigmodel.cn/api/coding/paas/v4"),
            vec![
                "https://open.bigmodel.cn/api/coding/paas/v4/models",
                "https://open.bigmodel.cn/api/coding/paas/v4/v1/models",
            ]
        );
        // 最长后缀优先:/api/anthropic 不能只剥掉 /anthropic 而留下残缺的 /api 根。
        assert_eq!(
            candidates("https://api.z.ai/api/anthropic"),
            vec![
                "https://api.z.ai/api/anthropic/v1/models",
                "https://api.z.ai/v1/models",
                "https://api.z.ai/models",
            ]
        );
        // 尾部斜杠与 /v1 不应产生重复候选。
        assert_eq!(
            candidates("https://api.example.com/v1/"),
            vec!["https://api.example.com/v1/models"]
        );
    }

    #[test]
    fn detects_version_and_compat_suffix_shapes() {
        assert!(ends_with_version_segment("https://x.com/v1"));
        assert!(ends_with_version_segment(
            "https://x.com/api/coding/paas/v4"
        ));
        assert!(!ends_with_version_segment("https://x.com/vX"));
        assert!(!ends_with_version_segment("https://x.com/anthropic"));
        assert_eq!(
            strip_compat_suffix("https://api.z.ai/api/anthropic"),
            Some("https://api.z.ai")
        );
        assert_eq!(strip_compat_suffix("https://api.z.ai/api"), None);
        // 剥离后只剩 scheme:// 时必须判定为不保留源,避免拼出畸形 URL。
        assert!(!keeps_origin("https://"));
        assert!(keeps_origin("https://api.z.ai"));
    }

    #[test]
    fn builds_public_catalog_candidates_from_api_root() {
        let url = validate_model_base_url(
            "https://agentrouter.org/anthropic",
            ModelDetectionPolicy::LocalUser,
        )
        .unwrap();
        assert_eq!(
            public_model_catalog_candidates(&url),
            vec!["https://agentrouter.org/api/pricing"]
        );

        let versioned =
            validate_model_base_url("https://gw.example.com/v1", ModelDetectionPolicy::LocalUser)
                .unwrap();
        assert_eq!(
            public_model_catalog_candidates(&versioned),
            vec!["https://gw.example.com/api/pricing"]
        );

        // 兜底候选必须与 base URL 同源(含端口),否则 PairedDevice 的 DNS 固定会失效。
        let ported = validate_model_base_url(
            "https://gw.example.com:8443/nested/anthropic",
            ModelDetectionPolicy::LocalUser,
        )
        .unwrap();
        assert_eq!(
            public_model_catalog_candidates(&ported),
            vec![
                "https://gw.example.com:8443/nested/api/pricing",
                "https://gw.example.com:8443/api/pricing",
            ]
        );

        // IPv6 字面量 host 需保留方括号,拼字符串会拼出畸形 URL。
        let ipv6 = validate_model_base_url(
            "http://[2001:db8::1]:9000/v1",
            ModelDetectionPolicy::LocalUser,
        )
        .unwrap();
        assert_eq!(
            public_model_catalog_candidates(&ipv6),
            vec!["http://[2001:db8::1]:9000/api/pricing"]
        );
    }

    #[test]
    fn parses_new_api_public_pricing_catalog() {
        let value = serde_json::json!({
            "data": [
                { "model_name": "claude-opus-5", "model_ratio": 1.0 },
                { "model_name": "claude-opus-4-8" },
                { "model_name": "claude-opus-5" },
                { "model_ratio": 2.0 }
            ]
        });

        assert_eq!(
            parse_public_catalog_model_ids(&value),
            vec!["claude-opus-4-8", "claude-opus-5"]
        );
        assert!(parse_public_catalog_model_ids(&serde_json::json!({ "data": [] })).is_empty());
    }

    #[test]
    fn flags_client_rejection_bodies_only() {
        assert!(looks_like_client_rejection(
            "HTTP 401: {\"error\":{\"message\":\"unauthorized client detected\"}}"
        ));
        assert!(looks_like_client_rejection("Invalid Client"));
        assert!(!looks_like_client_rejection(
            "HTTP 401: {\"error\":{\"message\":\"无效的令牌\"}}"
        ));
        assert!(!looks_like_client_rejection("HTTP 401: Invalid API Key"));
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
        assert_eq!(
            model_endpoint_candidates(&public),
            vec!["https://api.example.com/v1/models"]
        );
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
