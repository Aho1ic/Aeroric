//! 取 embedding 向量。
//!
//! 两种 provider:
//!
//! - **Ollama 原生** `POST {base}/api/embed`,body `{"model","input":[...]}`,
//!   回 `{"embeddings":[[...]]}`。本机跑,没有 key。
//! - **OpenAI 兼容** `POST {base}/v1/embeddings`,body `{"model","input":[...]}`,
//!   回 `{"data":[{"embedding":[...],"index":n}]}`。这一路覆盖 OpenAI 本身、
//!   LM Studio、vLLM、llama.cpp server、以及各家兼容网关。
//!
//! Ollama 自己也提供 `/v1/embeddings`,所以理论上一条路够用。留两条是为了错误
//! 信息:模型没 pull 时原生接口会直说,兼容层只给一个干巴巴的 404。
//!
//! ## 向量在入库前会被归一化
//!
//! sqlite-vec 的 `vec0` 默认按 L2 距离检索,而几乎所有 embedding 模型训练时用的
//! 是余弦相似度。把向量归一化到单位长度之后,L2 距离与余弦相似度是**单调等价**的
//! (‖a−b‖² = 2 − 2·cos(a,b)),于是按 L2 取最近邻和按余弦取最相似给出同一个排序。
//! 不归一化的话,长向量会因为模长而不是方向被判成远 —— 那种错误不会报,只会让
//! 检索结果悄悄变差。

use std::time::Duration;

use serde_json::{json, Value};

/// 单批最多送多少段文本。
///
/// 本机 Ollama 取小值:一批的显存占用是并发的主要来源,批太大在小显存机器上
/// 会直接 OOM 掉模型。远端按批次计费与限流,取大值省往返。
const OLLAMA_BATCH: usize = 16;
const OPENAI_BATCH: usize = 64;

/// 单批的超时。本机模型冷启动(第一次加载权重)能花掉几十秒,远端大批次也慢,
/// 所以给得比普通请求宽。
const BATCH_TIMEOUT: Duration = Duration::from_secs(180);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// 重试次数(总尝试次数 = 这个数)。
const MAX_ATTEMPTS: u32 = 3;

/// 哪一路 provider。
///
/// `Default` 是给 `app_settings::NotebookEmbeddingSettings` 的 `#[serde(default)]` 用的:
/// 老配置文件里没有这个字段时得落到本机 Ollama —— 那也是设置页出现之前前端硬编码的那个
/// 默认值,于是升级不改变任何人的既有行为。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmbedProvider {
    #[default]
    Ollama,
    OpenAi,
}

impl EmbedProvider {
    fn batch_size(self) -> usize {
        match self {
            EmbedProvider::Ollama => OLLAMA_BATCH,
            EmbedProvider::OpenAi => OPENAI_BATCH,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedConfig {
    pub provider: EmbedProvider,
    /// 形如 `http://127.0.0.1:11434` 或 `https://api.openai.com/v1`。
    /// 末尾斜杠与重复的 `/v1` 由 [`endpoint_for`] 归一。
    pub base_url: String,
    pub model: String,
    /// OpenAI 兼容一路的 key。Ollama 不需要。
    #[serde(default)]
    pub api_key: String,
}

/// 一次调用的失败原因。分类的用途是决定「重试还不还有意义」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedError {
    /// 配置本身不对(URL 非法、模型名空)。重试不会好转,该让用户改配置。
    Config(String),
    /// 上游拒绝且换次数不会变(401/403/404、模型不存在)。
    Rejected(String),
    /// 限流或上游临时故障(429/5xx)、网络层失败。值得重试。
    Transient(String),
    /// 回来的东西不是预期形状。当成永久失败 —— 重试只会拿到同一份垃圾。
    Malformed(String),
}

impl EmbedError {
    pub fn retryable(&self) -> bool {
        matches!(self, EmbedError::Transient(_))
    }

    pub fn message(&self) -> &str {
        match self {
            EmbedError::Config(m)
            | EmbedError::Rejected(m)
            | EmbedError::Transient(m)
            | EmbedError::Malformed(m) => m,
        }
    }
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// 校验并归一 base URL。
///
/// 拒绝带凭据的 URL:那种 URL 会把用户名密码带进日志和错误信息里。这里**不**
/// 拒绝 loopback 与私网 —— 本机 Ollama 正是这个用法,拦掉等于把主要场景判死。
/// (与 `app_settings::models` 里给配对设备用的 SSRF 闸门是刻意的差别,那条路
/// 面对的是不可信的请求来源,这条路是用户在自己机器上填自己的服务。)
pub fn validate_base_url(base_url: &str) -> Result<url::Url, EmbedError> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(EmbedError::Config("Embedding base URL is required".into()));
    }
    let url = url::Url::parse(trimmed)
        .map_err(|_| EmbedError::Config("Invalid embedding base URL".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(EmbedError::Config(
            "Embedding base URL must use http or https".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(EmbedError::Config(
            "Embedding base URL cannot contain credentials".into(),
        ));
    }
    if url.host_str().is_none() {
        return Err(EmbedError::Config(
            "Embedding base URL must include a host".into(),
        ));
    }
    Ok(url)
}

/// 拼出实际要打的地址。
///
/// 用户填 base URL 时带不带 `/v1` 都很常见(模型设置那边的历史就是这样),所以
/// OpenAI 一路要认已有的 `/v1` 而不是傻拼成 `/v1/v1/embeddings`。
pub fn endpoint_for(provider: EmbedProvider, base_url: &str) -> Result<String, EmbedError> {
    let url = validate_base_url(base_url)?;
    let base = url.as_str().trim_end_matches('/').to_string();
    Ok(match provider {
        EmbedProvider::Ollama => {
            // 有人会把 Ollama 的 base 填成 `.../v1`(照着 OpenAI 的样子)。原生
            // 接口不在 `/v1` 下,这里剥掉,免得只因为多了两个字符就 404。
            let root = base.trim_end_matches("/v1");
            format!("{root}/api/embed")
        }
        EmbedProvider::OpenAi => {
            if base.ends_with("/v1") {
                format!("{base}/embeddings")
            } else {
                format!("{base}/v1/embeddings")
            }
        }
    })
}

/// 构造请求体。
pub fn request_body(provider: EmbedProvider, model: &str, inputs: &[String]) -> Value {
    match provider {
        // 两家的 body 形状恰好一致(model + input 数组),但这是巧合而非契约 ——
        // 分开写,免得将来一方改了另一方跟着坏。
        EmbedProvider::Ollama => json!({ "model": model, "input": inputs }),
        EmbedProvider::OpenAi => json!({ "model": model, "input": inputs }),
    }
}

/// 从响应里取出向量。
///
/// 纯函数,因为这里是最容易出错的一步:各家兼容层的 `data` 顺序不保证与请求
/// 顺序一致,少一条或多一条都必须当成错误 —— 错位的向量会让检索结果看起来
/// 合理却指向别的段落,而那种 bug 没有任何报错。
pub fn parse_response(
    provider: EmbedProvider,
    body: &Value,
    expected: usize,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    let vectors = match provider {
        EmbedProvider::Ollama => {
            let list = body
                .get("embeddings")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    EmbedError::Malformed(embed_error_hint(body, "response has no `embeddings`"))
                })?;
            list.iter()
                .map(parse_vector)
                .collect::<Result<Vec<_>, _>>()?
        }
        EmbedProvider::OpenAi => {
            let list = body.get("data").and_then(Value::as_array).ok_or_else(|| {
                EmbedError::Malformed(embed_error_hint(body, "response has no `data`"))
            })?;
            // 按 `index` 归位。多数实现按序返回,但协议只保证 index 字段 ——
            // 依赖顺序是那种「平时对、偶尔悄悄错位」的 bug。
            let mut slots: Vec<Option<Vec<f32>>> = vec![None; list.len()];
            for (position, item) in list.iter().enumerate() {
                let vector = item
                    .get("embedding")
                    .ok_or_else(|| EmbedError::Malformed("response item has no `embedding`".into()))
                    .and_then(parse_vector)?;
                let index = item
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|i| i as usize)
                    // 没给 index 的实现退回位置序。
                    .unwrap_or(position);
                let slot = slots.get_mut(index).ok_or_else(|| {
                    EmbedError::Malformed(format!("response index {index} is out of range"))
                })?;
                if slot.is_some() {
                    return Err(EmbedError::Malformed(format!(
                        "response contains duplicate index {index}"
                    )));
                }
                *slot = Some(vector);
            }
            slots
                .into_iter()
                .enumerate()
                .map(|(index, slot)| {
                    slot.ok_or_else(|| {
                        EmbedError::Malformed(format!("response is missing index {index}"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    if vectors.len() != expected {
        return Err(EmbedError::Malformed(format!(
            "expected {expected} embeddings but got {}",
            vectors.len()
        )));
    }
    if let Some(empty) = vectors.iter().position(Vec::is_empty) {
        return Err(EmbedError::Malformed(format!(
            "embedding {empty} came back empty"
        )));
    }
    // 同一批里维度不一致说明上游把请求串了 —— 不能入库,vec0 会在插入时报错,
    // 而那时已经不知道是哪一批的问题了。
    let dim = vectors[0].len();
    if let Some(odd) = vectors.iter().position(|v| v.len() != dim) {
        return Err(EmbedError::Malformed(format!(
            "embedding {odd} has dimension {} but the batch is {dim}",
            vectors[odd].len()
        )));
    }
    Ok(vectors)
}

fn parse_vector(value: &Value) -> Result<Vec<f32>, EmbedError> {
    let array = value
        .as_array()
        .ok_or_else(|| EmbedError::Malformed("embedding is not an array".into()))?;
    array
        .iter()
        .map(|item| {
            item.as_f64()
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .ok_or_else(|| {
                    // NaN / Infinity 入库会让所有 KNN 查询的排序变成未定义。
                    EmbedError::Malformed("embedding contains a non-finite value".into())
                })
        })
        .collect()
}

/// 尽量从错误响应里挖出上游给的说明。挖不到就用兜底文案。
fn embed_error_hint(body: &Value, fallback: &str) -> String {
    for path in [&["error", "message"][..], &["error"][..], &["message"][..]] {
        let mut cursor = body;
        for key in path {
            match cursor.get(*key) {
                Some(next) => cursor = next,
                None => {
                    cursor = body;
                    break;
                }
            }
        }
        if let Some(text) = cursor.as_str() {
            if !text.trim().is_empty() {
                return text.trim().to_string();
            }
        }
    }
    fallback.to_string()
}

/// HTTP 状态码 → 错误分类。
pub fn classify_status(kind: &str, status: u16, detail: &str) -> EmbedError {
    let detail = if detail.trim().is_empty() {
        format!("{kind} request failed with status {status}")
    } else {
        detail.trim().to_string()
    };
    match status {
        // 429 是限流,退避后重试正是它想要的反应。
        429 => EmbedError::Transient(detail),
        // 5xx 是上游自己的问题,可能是暂时的。
        500..=599 => EmbedError::Transient(detail),
        // 其余 4xx:key 不对、模型不存在、请求形状不合 —— 重试拿到的是同一个答案。
        _ => EmbedError::Rejected(detail),
    }
}

/// 把向量归一化到单位长度。
///
/// 模长为 0 的向量除不了 —— 原样返回而不是产出 NaN。那种向量在检索里没有方向
/// 可言(与任何查询的余弦都是 0/0),但让它安静地待着比污染整张表好。
pub fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm <= f32::EPSILON || !norm.is_finite() {
        return;
    }
    for value in vector.iter_mut() {
        *value /= norm;
    }
}

/// 把输入切成若干批。
pub fn batches(provider: EmbedProvider, count: usize) -> Vec<std::ops::Range<usize>> {
    let size = provider.batch_size().max(1);
    (0..count)
        .step_by(size)
        .map(|start| start..(start + size).min(count))
        .collect()
}

/// 构造 HTTP 客户端。形状与 `agent_tools::http_client` 一致(代理 + UA),差别是
/// no_proxy 会兜住 loopback —— 本机 Ollama 被推去代理的话会从「能用」变「连不上」。
pub(super) fn client(timeout: Duration) -> Result<reqwest::Client, EmbedError> {
    let settings = crate::app_settings::load_settings_internal();
    let mut builder = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(timeout)
        // embedding 端点不该重定向。跟随重定向会把 Authorization 头带去别的 host。
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("Aeroric/{}", env!("CARGO_PKG_VERSION")));
    let proxy_url = settings.proxy_settings.url.trim();
    if !proxy_url.is_empty() {
        let mut proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|error| EmbedError::Config(format!("Invalid proxy configuration: {error}")))?;
        let username = settings.proxy_settings.username.trim();
        if !username.is_empty() {
            proxy = proxy.basic_auth(username, settings.proxy_settings.password.trim());
        }
        proxy = proxy.no_proxy(reqwest::NoProxy::from_string(&no_proxy_rules(
            &settings.proxy_settings.no_proxy,
        )));
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|error| EmbedError::Config(format!("Cannot create HTTP client: {error}")))
}

/// 在用户规则之外追加 loopback 与私网。与 `app_settings` 里
/// `detect_no_proxy_rules` 同一份理由:本机服务走代理会连不上。
fn no_proxy_rules(user_rules: &str) -> String {
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

/// 退避时长。指数,但封顶 —— 索引是用户看着进度条等的操作,退避到几十秒会
/// 让它看起来像卡死了。
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64 << attempt.min(3))
}

/// 送一批文本,拿回等长的向量列表(已归一化)。
///
/// `cancel` 每次重试前检查一次:退避期间用户点了取消,不该继续等下去。
///
/// `+ Sync` 与 [`super::index::ProgressSink`] 同一个理由:这个引用要跨 await 活着,
/// 而 `&T` 只有在 `T: Sync` 时才是 `Send`。少了它整条调用链的 future 都不是 `Send`,
/// `#[tauri::command]` 用不了。
pub async fn embed_batch(
    config: &EmbedConfig,
    inputs: &[String],
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<Vec<f32>>, EmbedError> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    if config.model.trim().is_empty() {
        return Err(EmbedError::Config("Embedding model is required".into()));
    }
    let endpoint = endpoint_for(config.provider, &config.base_url)?;
    let body = request_body(config.provider, config.model.trim(), inputs);
    let client = client(BATCH_TIMEOUT)?;
    let key = config.api_key.trim();

    let mut last = EmbedError::Transient("embedding request was never attempted".into());
    for attempt in 0..MAX_ATTEMPTS {
        if cancel() {
            return Err(EmbedError::Transient("cancelled".into()));
        }
        if attempt > 0 {
            tokio::time::sleep(backoff(attempt - 1)).await;
            if cancel() {
                return Err(EmbedError::Transient("cancelled".into()));
            }
        }
        let mut request = client.post(&endpoint).json(&body);
        // Ollama 本机没有 key。填了也送 —— 有人在 Ollama 前面挂了带鉴权的反代。
        if !key.is_empty() {
            request = request.bearer_auth(key);
        }
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                // 先取文本再解析:上游报错时给的往往不是 JSON(网关的 HTML 错误页),
                // 直接 `.json()` 会把「502 网关挂了」变成一个语法错误。
                let text = response.text().await.unwrap_or_default();
                if !status.is_success() {
                    let detail = serde_json::from_str::<Value>(&text)
                        .map(|value| embed_error_hint(&value, ""))
                        .unwrap_or_default();
                    let detail = if detail.is_empty() {
                        truncate_detail(&text)
                    } else {
                        detail
                    };
                    last = classify_status("embedding", status.as_u16(), &detail);
                    if !last.retryable() {
                        return Err(last);
                    }
                    continue;
                }
                let value: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(error) => {
                        return Err(EmbedError::Malformed(format!(
                            "embedding response is not JSON: {error}"
                        )))
                    }
                };
                let mut vectors = parse_response(config.provider, &value, inputs.len())?;
                for vector in vectors.iter_mut() {
                    normalize(vector);
                }
                return Ok(vectors);
            }
            Err(error) => {
                // 网络层失败(DNS / 建连 / 超时)一律可重试。
                // 注意不要把 error 直接拼进消息里 —— reqwest 的 Display 会带上
                // URL,而 URL 在这条路上不含凭据,但下游日志仍然不需要它。
                last = EmbedError::Transient(network_error_hint("embedding", &error));
                continue;
            }
        }
    }
    Err(last)
}

pub(super) fn truncate_detail(text: &str) -> String {
    const LIMIT: usize = 200;
    let trimmed = text.trim();
    if trimmed.chars().count() <= LIMIT {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(LIMIT).collect();
    format!("{head}…")
}

pub(super) fn network_error_hint(kind: &str, error: &reqwest::Error) -> String {
    if error.is_timeout() {
        return format!("{kind} request timed out");
    }
    if error.is_connect() {
        return format!("cannot connect to the {kind} endpoint");
    }
    format!("{kind} request failed")
}

/// 探测维度:拿一小段文本走一次真实调用,量回来的长度。
///
/// 不硬编维度表:同名模型在不同 provider 上维度可以不同(量化版、社区改版),
/// 而维度写错的表现是入库时整批报错。探一次的成本是一次最小请求。
pub async fn probe_dimension(config: &EmbedConfig) -> Result<usize, EmbedError> {
    let never = || false;
    let vectors = embed_batch(config, &["dimension probe".to_string()], &never).await?;
    let dim = vectors
        .first()
        .map(Vec::len)
        .ok_or_else(|| EmbedError::Malformed("dimension probe returned nothing".into()))?;
    if dim == 0 {
        return Err(EmbedError::Malformed(
            "embedding model reported zero dimensions".into(),
        ));
    }
    Ok(dim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ollama_native_endpoint() {
        assert_eq!(
            endpoint_for(EmbedProvider::Ollama, "http://127.0.0.1:11434").unwrap(),
            "http://127.0.0.1:11434/api/embed"
        );
    }

    #[test]
    fn strips_v1_suffix_for_ollama() {
        // 照着 OpenAI 填 base URL 很常见;原生接口不在 /v1 下。
        assert_eq!(
            endpoint_for(EmbedProvider::Ollama, "http://127.0.0.1:11434/v1").unwrap(),
            "http://127.0.0.1:11434/api/embed"
        );
    }

    #[test]
    fn appends_v1_for_openai_when_missing() {
        assert_eq!(
            endpoint_for(EmbedProvider::OpenAi, "https://api.openai.com").unwrap(),
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn does_not_double_v1_for_openai() {
        assert_eq!(
            endpoint_for(EmbedProvider::OpenAi, "https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn tolerates_trailing_slashes() {
        assert_eq!(
            endpoint_for(EmbedProvider::OpenAi, "https://api.openai.com/v1/").unwrap(),
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn allows_loopback_base_urls() {
        // 本机 Ollama 是主要场景,拦掉 loopback 等于把它判死。
        assert!(validate_base_url("http://127.0.0.1:11434").is_ok());
        assert!(validate_base_url("http://localhost:11434").is_ok());
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(matches!(
            validate_base_url("file:///etc/passwd"),
            Err(EmbedError::Config(_))
        ));
    }

    #[test]
    fn rejects_credentials_in_url() {
        // 带凭据的 URL 会把密码带进日志和错误信息。
        assert!(matches!(
            validate_base_url("http://user:pass@example.com"),
            Err(EmbedError::Config(_))
        ));
    }

    #[test]
    fn rejects_empty_base_url() {
        assert!(matches!(
            validate_base_url("   "),
            Err(EmbedError::Config(_))
        ));
    }

    #[test]
    fn parses_ollama_response() {
        let body = json!({ "embeddings": [[1.0, 0.0], [0.0, 1.0]] });
        let vectors = parse_response(EmbedProvider::Ollama, &body, 2).unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    }

    #[test]
    fn parses_openai_response() {
        let body = json!({
            "data": [
                { "embedding": [1.0, 0.0], "index": 0 },
                { "embedding": [0.0, 1.0], "index": 1 }
            ]
        });
        let vectors = parse_response(EmbedProvider::OpenAi, &body, 2).unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    }

    #[test]
    fn reorders_openai_response_by_index() {
        // 错位的向量会让检索指向别的段落,而且不报任何错。
        let body = json!({
            "data": [
                { "embedding": [0.0, 1.0], "index": 1 },
                { "embedding": [1.0, 0.0], "index": 0 }
            ]
        });
        let vectors = parse_response(EmbedProvider::OpenAi, &body, 2).unwrap();
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    }

    #[test]
    fn falls_back_to_position_when_index_is_absent() {
        let body = json!({ "data": [{ "embedding": [1.0] }, { "embedding": [2.0] }] });
        let vectors = parse_response(EmbedProvider::OpenAi, &body, 2).unwrap();
        assert_eq!(vectors, vec![vec![1.0], vec![2.0]]);
    }

    #[test]
    fn rejects_duplicate_indices() {
        let body = json!({
            "data": [
                { "embedding": [1.0], "index": 0 },
                { "embedding": [2.0], "index": 0 }
            ]
        });
        assert!(matches!(
            parse_response(EmbedProvider::OpenAi, &body, 2),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_out_of_range_index() {
        let body = json!({ "data": [{ "embedding": [1.0], "index": 7 }] });
        assert!(matches!(
            parse_response(EmbedProvider::OpenAi, &body, 1),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_wrong_count() {
        // 少一条就意味着某段笔记没有向量,而调用方会按位置对号入座。
        let body = json!({ "embeddings": [[1.0, 0.0]] });
        assert!(matches!(
            parse_response(EmbedProvider::Ollama, &body, 2),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_mixed_dimensions_in_a_batch() {
        let body = json!({ "embeddings": [[1.0, 0.0], [1.0, 0.0, 0.0]] });
        assert!(matches!(
            parse_response(EmbedProvider::Ollama, &body, 2),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_empty_vectors() {
        let body = json!({ "embeddings": [[]] });
        assert!(matches!(
            parse_response(EmbedProvider::Ollama, &body, 1),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_non_finite_values() {
        // NaN 入库会让所有 KNN 查询的排序变成未定义。JSON 里没有 NaN 字面量,
        // 但字符串 "NaN" 这类东西会从宽松的兼容层里回来。
        let body = json!({ "embeddings": [["NaN", 1.0]] });
        assert!(matches!(
            parse_response(EmbedProvider::Ollama, &body, 1),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn surfaces_upstream_error_message() {
        let body = json!({ "error": { "message": "model \"nope\" not found" } });
        let error = parse_response(EmbedProvider::Ollama, &body, 1).unwrap_err();
        assert!(error.message().contains("not found"), "{error}");
    }

    #[test]
    fn surfaces_flat_error_string() {
        let body = json!({ "error": "model not found, try pulling it first" });
        let error = parse_response(EmbedProvider::Ollama, &body, 1).unwrap_err();
        assert!(error.message().contains("pulling it first"), "{error}");
    }

    #[test]
    fn classifies_rate_limit_as_retryable() {
        assert!(classify_status("embedding", 429, "slow down").retryable());
    }

    #[test]
    fn classifies_server_errors_as_retryable() {
        assert!(classify_status("embedding", 500, "boom").retryable());
        assert!(classify_status("embedding", 503, "unavailable").retryable());
    }

    #[test]
    fn classifies_auth_failures_as_permanent() {
        // 重试拿到的是同一个 401,只会把进度条拖长。
        assert!(!classify_status("embedding", 401, "bad key").retryable());
        assert!(!classify_status("embedding", 404, "no such model").retryable());
    }

    #[test]
    fn normalizes_to_unit_length() {
        let mut vector = vec![3.0, 4.0];
        normalize(&mut vector);
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "{norm}");
    }

    #[test]
    fn normalize_preserves_direction() {
        // L2 与余弦的单调等价靠的是方向不变。
        let mut vector = vec![2.0, 0.0];
        normalize(&mut vector);
        assert!((vector[0] - 1.0).abs() < 1e-6);
        assert!(vector[1].abs() < 1e-6);
    }

    #[test]
    fn normalize_leaves_zero_vector_alone() {
        // 除以 0 会产出 NaN 并污染整张表。
        let mut vector = vec![0.0, 0.0];
        normalize(&mut vector);
        assert_eq!(vector, vec![0.0, 0.0]);
    }

    #[test]
    fn batches_cover_every_input_exactly_once() {
        let ranges = batches(EmbedProvider::Ollama, 40);
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, 40);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "批次之间漏了或重了");
        }
        let covered: usize = ranges.iter().map(|r| r.len()).sum();
        assert_eq!(covered, 40);
    }

    #[test]
    fn batches_respect_provider_size() {
        assert_eq!(batches(EmbedProvider::Ollama, 40).len(), 3);
        assert_eq!(batches(EmbedProvider::OpenAi, 40).len(), 1);
    }

    #[test]
    fn batches_handle_empty_input() {
        assert!(batches(EmbedProvider::Ollama, 0).is_empty());
    }

    #[test]
    fn backoff_grows_then_caps() {
        assert!(backoff(1) > backoff(0));
        // 封顶,否则进度条看起来像卡死了。
        assert_eq!(backoff(6), backoff(3));
    }

    #[test]
    fn no_proxy_rules_keep_user_entries_and_add_loopback() {
        let rules = no_proxy_rules("example.com");
        assert!(rules.contains("example.com"));
        assert!(rules.contains("127.0.0.1"));
        assert!(rules.contains("localhost"));
    }

    #[test]
    fn no_proxy_rules_do_not_duplicate() {
        let rules = no_proxy_rules("localhost");
        assert_eq!(rules.matches("localhost").count(), 1);
    }

    #[test]
    fn truncates_long_error_bodies() {
        // 网关的 HTML 错误页整页拼进错误信息里没有意义。
        let long = "x".repeat(500);
        let detail = truncate_detail(&long);
        assert!(detail.chars().count() <= 201, "{}", detail.chars().count());
    }

    #[test]
    fn truncate_does_not_split_multibyte_chars() {
        let long = "错".repeat(500);
        let detail = truncate_detail(&long);
        assert!(detail.starts_with('错'));
    }

    #[tokio::test]
    async fn empty_input_short_circuits_without_a_request() {
        // base URL 是垃圾也不该报错 —— 根本不该发请求。
        let config = EmbedConfig {
            provider: EmbedProvider::Ollama,
            base_url: "not a url".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let never = || false;
        assert_eq!(
            embed_batch(&config, &[], &never).await.unwrap(),
            Vec::<Vec<f32>>::new()
        );
    }

    #[tokio::test]
    async fn rejects_empty_model_before_requesting() {
        let config = EmbedConfig {
            provider: EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:11434".into(),
            model: "  ".into(),
            api_key: String::new(),
        };
        let never = || false;
        let error = embed_batch(&config, &["x".into()], &never)
            .await
            .unwrap_err();
        assert!(matches!(error, EmbedError::Config(_)), "{error}");
    }

    #[tokio::test]
    async fn honours_cancellation_before_the_first_request() {
        let config = EmbedConfig {
            provider: EmbedProvider::Ollama,
            base_url: "http://127.0.0.1:1".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let always = || true;
        let error = embed_batch(&config, &["x".into()], &always)
            .await
            .unwrap_err();
        assert_eq!(error.message(), "cancelled");
    }
}
