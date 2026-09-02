//! 模型列表探测的网络部分。
//!
//! 从 `app_settings.rs` 整块搬出来,内容一行没改。构造探测用的 HTTP 客户端、
//! 按候选链逐个试、地址级故障转移都在这里。判定策略那个 enum
//! (`ModelDetectionPolicy`)在兄弟模块 `models.rs`,靠 `use super::*;` 取。
//!
//! 这里的代理处理有两条和别处不同的规则,原注释都在:只有 `LocalUser` 会套
//! 应用内代理(`PairedDevice` 必须直连,否则代理自己解析域名会绕开
//! `resolve_to_addrs` 钉死的「已过滤私网」地址,桌面端就成了探内网的跳板);
//! 以及 `no_proxy` 会额外追加 loopback 与私网网段。

use super::*;

/// 探测客户端最多尝试几个上游地址。
///
/// 每次尝试都要重跑整条候选链,而共享 deadline 是固定的;3 个足以跨过「几个地址里
/// 坏一个」的常见情形,再多只是把预算耗在同一个不可用的上游上。
pub(super) const MODEL_DETECT_MAX_ADDRESS_ATTEMPTS: usize = 3;

/// 探测客户端是否要套应用内代理。
///
/// 仅 [`ModelDetectionPolicy::LocalUser`] 走代理。`PairedDevice` 保持直连是刻意的:
/// 那条路径靠 `resolve_to_addrs` 把域名钉死在「已过滤掉私网」的地址上,以防配对手机
/// 拿桌面端当跳板探内网;而代理会自己解析域名,钉死随之失效,闸门就形同虚设。
///
/// 调用方还用它决定「是否做地址故障转移」:走代理时目标地址由代理决定,
/// 本机解析出的地址在那条路径上根本用不到(`resolve_to_addrs` 只影响直连)。
pub(super) fn detect_proxy_applies(settings: &ProxySettings, policy: ModelDetectionPolicy) -> bool {
    matches!(policy, ModelDetectionPolicy::LocalUser)
        && !normalize_proxy_url(&settings.url).is_empty()
}

/// 给探测客户端套上应用内代理设置。
///
/// 形状与 `agent_tools.rs::http_client()` 一致(`Proxy::all` + `basic_auth` + `NoProxy`),
/// 差别只在 `no_proxy` 会额外追加 loopback / 私网 —— 详见 [`detect_no_proxy_rules`]。
pub(super) fn apply_detect_proxy(
    builder: reqwest::ClientBuilder,
    settings: &ProxySettings,
    policy: ModelDetectionPolicy,
) -> Result<reqwest::ClientBuilder, String> {
    if !detect_proxy_applies(settings, policy) {
        return Ok(builder);
    }
    let mut proxy = reqwest::Proxy::all(normalize_proxy_url(&settings.url))
        .map_err(|error| format!("Invalid proxy configuration: {error}"))?;
    let username = settings.username.trim();
    if !username.is_empty() {
        proxy = proxy.basic_auth(username, settings.password.trim());
    }
    proxy = proxy.no_proxy(reqwest::NoProxy::from_string(&detect_no_proxy_rules(
        &settings.no_proxy,
    )));
    Ok(builder.proxy(proxy))
}

/// 在用户的 `no_proxy` 之外追加 loopback 与私网网段。
///
/// 本机 base URL(Ollama、应用自带的 local_router)被推去代理后会从「能用」变成
/// 「连不上」—— 代理通常不会把请求转回发起方的 loopback。用户没有理由为了让
/// 模型探测工作而手写这些例外,所以在这里兜住。
pub(super) fn detect_no_proxy_rules(user_rules: &str) -> String {
    const LOCAL_RULES: &[&str] = &[
        "127.0.0.1",
        "::1",
        "localhost",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
    ];
    let mut rules: Vec<&str> = user_rules
        .split(',')
        .map(str::trim)
        .filter(|rule| !rule.is_empty())
        .collect();
    for rule in LOCAL_RULES {
        if !rules
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(rule))
        {
            rules.push(rule);
        }
    }
    rules.join(",")
}

/// 构造一个探测客户端。`pinned` 非空时把 host 钉在这些地址上(地址故障转移用)。
pub(super) fn build_detect_client(
    base_url: &url::Url,
    proxy_settings: &ProxySettings,
    policy: ModelDetectionPolicy,
    pinned: &[SocketAddr],
) -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder()
        // 候选端点串行探测,总预算在 models.rs 内按 deadline 控制,
        // 这里不再设客户端级总超时,避免第一个慢候选耗尽后续机会。
        .redirect(reqwest::redirect::Policy::none())
        // 黑洞地址(TCP 通、TLS 不返回)要在这里被判死,而不是拖到请求超时。
        .connect_timeout(MODEL_DETECT_CONNECT_TIMEOUT);
    let mut builder = apply_detect_proxy(builder, proxy_settings, policy)?;
    if !pinned.is_empty() {
        let host = base_url
            .host_str()
            .ok_or_else(|| "Base URL must include a host".to_string())?;
        // 候选端点与 base URL 同源(仅替换 path),因此固定解析对全部候选生效。
        builder = builder.resolve_to_addrs(host, pinned);
    }
    builder.build().map_err(|error| error.to_string())
}

pub(super) async fn detect_agent_models_with_policy(
    kind: AgentSetupKind,
    base_url: String,
    api_key: String,
    policy: ModelDetectionPolicy,
) -> Result<AgentModels, String> {
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err("API key is required".to_string());
    }
    let base_url = validate_model_base_url(&base_url, policy)?;

    let resolved_addresses = if matches!(policy, ModelDetectionPolicy::PairedDevice) {
        Some(resolve_remote_model_addresses(&base_url).await?)
    } else {
        None
    };
    // 有锁 + 文件 IO,不能在 async 里直接阻塞(与 `list_agent_models` 一致)。
    let proxy_settings = tokio::task::spawn_blocking(|| load_settings_internal().proxy_settings)
        .await
        .map_err(|error| error.to_string())?;

    let pinned = resolved_addresses.clone().unwrap_or_default();

    // 地址故障转移只在「直连 + 本机用户」时有意义:代理路径的目标地址由代理决定,
    // PairedDevice 路径已经把全部允许的地址一次性钉好了。
    let failover_addresses =
        if detect_proxy_applies(&proxy_settings, policy) || resolved_addresses.is_some() {
            Vec::new()
        } else {
            resolve_local_model_addresses(&base_url).await
        };

    detect_models_over_http(
        &kind,
        &base_url,
        &api_key,
        policy,
        &proxy_settings,
        &pinned,
        &failover_addresses,
    )
    .await
}

/// 模型探测的网络部分:客户端构造 + 候选探测 + 地址故障转移 + 公开目录兜底。
///
/// 与 [`detect_agent_models_with_policy`] 分开是为了让测试能注入 `proxy_settings` 与
/// 地址列表,而不必读写真实的 `~/.aeroric/settings.json`。
pub(super) async fn detect_models_over_http(
    kind: &AgentSetupKind,
    base_url: &url::Url,
    api_key: &str,
    policy: ModelDetectionPolicy,
    proxy_settings: &ProxySettings,
    pinned: &[SocketAddr],
    failover_addresses: &[SocketAddr],
) -> Result<AgentModels, String> {
    let candidates = model_endpoint_candidates(base_url);

    // 每次尝试钉住的地址。
    //
    // 有 `failover_addresses` 时**每一轮都钉一个具体地址**,而不是先来一轮不钉的:
    // 不钉的那轮由 hyper 自己挑地址,失败后我们无从得知它挑了谁,只能从头再试一遍
    // 同一批地址 —— 首地址正是黑洞时,那 4s 建连超时会白付两次。
    let attempts: Vec<&[SocketAddr]> = if failover_addresses.len() > 1 {
        failover_addresses
            .iter()
            .take(MODEL_DETECT_MAX_ADDRESS_ATTEMPTS)
            .map(std::slice::from_ref)
            .collect()
    } else {
        vec![pinned]
    };

    // 全部尝试共享同一个 deadline,总等待不随重试次数线性放大。
    let deadline = std::time::Instant::now() + MODEL_DETECT_TOTAL_BUDGET;
    let mut detect = build_detect_client(base_url, proxy_settings, policy, attempts[0])?;
    let mut failures = DetectionFailures::default();
    let mut detected = Err("Model detection failed: no endpoint candidates".to_string());

    for (index, addresses) in attempts.iter().enumerate() {
        if index > 0 {
            if std::time::Instant::now() >= deadline {
                break;
            }
            detect = build_detect_client(base_url, proxy_settings, policy, addresses)?;
        }
        let mut attempt_failures = DetectionFailures::default();
        detected = fetch_agent_model_json_from_candidates(
            &detect,
            &candidates,
            kind,
            api_key,
            &mut attempt_failures,
            deadline,
        )
        .await;
        // 这一轮的结论覆盖上一轮:它才是最终要报给用户的原因。
        failures = attempt_failures;
        // 拿到响应(成功或上游明确拒绝)就停 —— 换地址只对传输层失败有意义。
        // 某个 A 记录接受 TCP 却让 TLS 卡死时,hyper 不会自己换地址,只能由这里驱动。
        if detected.is_ok() || !failures.transport || failures.auth {
            break;
        }
    }

    let models = match detected {
        Ok((value, _endpoint)) => parse_model_ids(value),
        Err(error) => {
            // 只有在「所有候选都是端点不存在」时才退到公开目录:出现过上游拒绝就必须
            // 把错误报出去,否则会把 API Key 无效伪装成一份不可用的模型列表。
            // 传输层失败同理不该兜底 —— 网络都没通,公开目录也拿不到可信结果。
            if failures.auth || failures.transport {
                return Err(error);
            }
            let public = fetch_public_model_catalog(
                &detect,
                &public_model_catalog_candidates(base_url),
                kind,
            )
            .await;
            match public {
                Some(models) if !models.is_empty() => models,
                _ => return Err(error),
            }
        }
    };

    // 复用最终成功的那个客户端:否则余额查询会重新解析域名,可能又踩到黑洞地址。
    let balance = fetch_agent_balance(&detect, base_url.as_str(), api_key).await;
    Ok(AgentModels {
        models,
        balance,
        reasoning_effort: None,
        reasoning_speed: None,
    })
}
