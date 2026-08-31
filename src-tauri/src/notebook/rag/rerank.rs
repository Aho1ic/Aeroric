//! 重排:把混合检索的候选交给 cross-encoder 精排。
//!
//! 融合出来的名次只看「查询和块各自的向量有多近」与「关键词命中得多好」,两者都
//! 不读完整的查询-文档配对。cross-encoder 读的是配对本身,在前 20 名里挑前 5 名
//! 这种场景上明显更准 —— 代价是一次网络往返,所以它是可选的。
//!
//! 协议只做「Cohere 兼容」这一种:Cohere 官方、infinity-emb、TEI 的 `/rerank` 都
//! 遵循同一份 schema。
//!
//! ```text
//! POST {base_url}/v1/rerank
//! Authorization: Bearer …
//! { model, query, documents: [...], top_n }
//! → { results: [{ index, relevance_score }] }
//! ```
//!
//! Ollama 没有官方的 rerank 端点。想用本地模型跑重排的话在外面套一层 infinity-emb,
//! 再把 base_url 指过去。
//!
//! ## 与 Markio 的差异
//!
//! Markio 的 `rerank_blocking` 在函数里 `Builder::new_current_thread().build()` 造了
//! 一个新 runtime 再 `block_on`。那在异步上下文里调用会直接 panic(「Cannot start a
//! runtime from within a runtime」),而检索本身是异步的。这里就是 `async fn`。
//!
//! 另外它信任服务端返回的 `index`:重复的下标会让同一个块在结果里出现两次,而
//! `docs.get(idx).cloned()` 不报错。这里校验下标唯一且在范围内。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::embed::{self, EmbedError};

/// 重排的超时。比 embedding 短:它只处理几十条候选,慢到这个地步说明服务有问题,
/// 而检索是用户在等的交互路径。
const RERANK_TIMEOUT: Duration = Duration::from_secs(30);

/// 一次请求最多送多少条候选。
///
/// cross-encoder 的开销随候选数线性增长,而重排的收益集中在前几十条 —— 送 500 条
/// 进去只会让用户等着。
pub const MAX_DOCUMENTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankConfig {
    /// 目前只有 `cohere`(Cohere 官方 + 任何 cohere 兼容服务)。
    pub provider: String,
    pub model: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

#[derive(Debug, Serialize)]
struct CohereDoc<'a> {
    text: &'a str,
}

#[derive(Debug, Serialize)]
struct CohereRequest<'a> {
    model: &'a str,
    query: &'a str,
    documents: Vec<CohereDoc<'a>>,
    top_n: usize,
    return_documents: bool,
}

/// 一条重排结果:候选在入参里的下标 + 相关度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ranked {
    pub index: usize,
    pub score: f32,
}

/// 拼出 rerank 端点。`/v1` 重复与末尾斜杠都归一掉。
pub fn endpoint_for(base_url: &str) -> Result<String, EmbedError> {
    let parsed = embed::validate_base_url(base_url)?;
    let trimmed = parsed.as_str().trim_end_matches('/').to_string();
    if trimmed.ends_with("/v1") {
        Ok(format!("{trimmed}/rerank"))
    } else {
        Ok(format!("{trimmed}/v1/rerank"))
    }
}

/// 解析响应。
///
/// `count` 是送出去的候选数,用来校验下标 —— 服务端给回越界或重复的 `index` 时必须
/// 报错而不是静默取错块:重复下标会让同一个块在结果里出现两次,越界下标会让某个
/// 候选凭空消失。
pub fn parse_response(body: &serde_json::Value, count: usize) -> Result<Vec<Ranked>, EmbedError> {
    let results = body
        .get("results")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            EmbedError::Malformed("rerank response has no `results` array".to_string())
        })?;
    let mut out: Vec<Ranked> = Vec::with_capacity(results.len());
    let mut seen = vec![false; count];
    for item in results {
        let index = item
            .get("index")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| {
                EmbedError::Malformed("rerank result has no numeric `index`".to_string())
            })? as usize;
        if index >= count {
            return Err(EmbedError::Malformed(format!(
                "rerank returned index {index} for a request of {count} documents"
            )));
        }
        if std::mem::replace(&mut seen[index], true) {
            return Err(EmbedError::Malformed(format!(
                "rerank returned index {index} more than once"
            )));
        }
        let score = item
            .get("relevance_score")
            .and_then(|value| value.as_f64())
            .ok_or_else(|| {
                EmbedError::Malformed("rerank result has no numeric `relevance_score`".to_string())
            })? as f32;
        if !score.is_finite() {
            return Err(EmbedError::Malformed(
                "rerank returned a non-finite score".to_string(),
            ));
        }
        out.push(Ranked { index, score });
    }
    // 按分数倒序。服务端一般已经排好,但协议没有保证。
    // 同分按下标升序兜底,免得顺序在两次调用间漂。
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.index.cmp(&b.index))
    });
    Ok(out)
}

/// 精排 `documents`,返回按相关度倒序的下标。
///
/// 失败时返回 `Err`,由调用方决定是不是回退到融合出来的原始名次 —— 重排是可选的
/// 增强,它挂掉不该让整次检索失败。
pub async fn rerank(
    config: &RerankConfig,
    query: &str,
    documents: &[String],
    top_n: usize,
) -> Result<Vec<Ranked>, EmbedError> {
    if documents.is_empty() || top_n == 0 {
        return Ok(Vec::new());
    }
    if config.provider != "cohere" {
        return Err(EmbedError::Config(format!(
            "Unsupported reranker provider `{}` (only cohere-compatible is supported)",
            config.provider
        )));
    }
    if config.model.trim().is_empty() {
        return Err(EmbedError::Config(
            "Reranker model is not configured".to_string(),
        ));
    }
    let sent = documents.len().min(MAX_DOCUMENTS);
    let url = endpoint_for(&config.base_url)?;
    let payload = CohereRequest {
        model: config.model.trim(),
        query,
        documents: documents[..sent]
            .iter()
            .map(|text| CohereDoc { text })
            .collect(),
        top_n: top_n.min(sent),
        return_documents: false,
    };

    let client = embed::client(RERANK_TIMEOUT)?;
    let mut request = client.post(&url).json(&payload);
    let key = config.api_key.trim();
    if !key.is_empty() {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|error| EmbedError::Transient(embed::network_error_hint("rerank", &error)))?;
    let status = response.status();
    // 先读文本再解析:上游报错时给的常常是 HTML,直接 json() 会把「502 网关」变成
    // 一句「解析失败」,真正的原因就丢了。
    let text = response
        .text()
        .await
        .map_err(|error| EmbedError::Transient(embed::network_error_hint("rerank", &error)))?;
    if !status.is_success() {
        return Err(embed::classify_status(
            "rerank",
            status.as_u16(),
            &embed::truncate_detail(&text),
        ));
    }
    let body: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        EmbedError::Malformed(format!(
            "Cannot parse rerank response: {error} ({})",
            embed::truncate_detail(&text)
        ))
    })?;
    let mut ranked = parse_response(&body, sent)?;
    ranked.truncate(top_n);
    Ok(ranked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> RerankConfig {
        RerankConfig {
            provider: "cohere".to_string(),
            model: "rerank-v3".to_string(),
            base_url: "https://api.cohere.com".to_string(),
            api_key: "k".to_string(),
        }
    }

    #[test]
    fn endpoint_appends_v1_when_absent() {
        assert_eq!(
            endpoint_for("https://api.cohere.com").expect("endpoint"),
            "https://api.cohere.com/v1/rerank"
        );
    }

    #[test]
    fn endpoint_does_not_double_v1() {
        assert_eq!(
            endpoint_for("https://host/v1").expect("endpoint"),
            "https://host/v1/rerank"
        );
        assert_eq!(
            endpoint_for("https://host/v1/").expect("endpoint"),
            "https://host/v1/rerank"
        );
    }

    #[test]
    fn endpoint_allows_loopback() {
        // 本地套一层 infinity-emb 是推荐用法,不能把 127.0.0.1 拒掉。
        assert!(endpoint_for("http://127.0.0.1:7997").is_ok());
    }

    #[test]
    fn endpoint_rejects_a_bad_base_url() {
        assert!(endpoint_for("not a url").is_err());
        assert!(endpoint_for("ftp://host").is_err());
    }

    #[test]
    fn parses_results_and_sorts_by_score() {
        let body = json!({
            "results": [
                {"index": 0, "relevance_score": 0.1},
                {"index": 2, "relevance_score": 0.9},
                {"index": 1, "relevance_score": 0.5},
            ]
        });
        let ranked = parse_response(&body, 3).expect("parse");
        assert_eq!(
            ranked.iter().map(|r| r.index).collect::<Vec<_>>(),
            vec![2, 1, 0]
        );
    }

    #[test]
    fn ties_break_by_index() {
        let body = json!({
            "results": [
                {"index": 2, "relevance_score": 0.5},
                {"index": 0, "relevance_score": 0.5},
            ]
        });
        let ranked = parse_response(&body, 3).expect("parse");
        assert_eq!(
            ranked.iter().map(|r| r.index).collect::<Vec<_>>(),
            vec![0, 2]
        );
    }

    #[test]
    fn rejects_a_duplicated_index() {
        // 信任下标会让同一个块在结果里出现两次。
        let body = json!({
            "results": [
                {"index": 1, "relevance_score": 0.9},
                {"index": 1, "relevance_score": 0.2},
            ]
        });
        assert!(matches!(
            parse_response(&body, 3),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_an_out_of_range_index() {
        let body = json!({"results": [{"index": 7, "relevance_score": 0.9}]});
        assert!(matches!(
            parse_response(&body, 3),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_a_non_finite_score() {
        // NaN 会让排序的全序关系失效。
        let body = json!({"results": [{"index": 0, "relevance_score": "nope"}]});
        assert!(matches!(
            parse_response(&body, 1),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_a_response_without_results() {
        let body = json!({"data": []});
        assert!(matches!(
            parse_response(&body, 1),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_a_result_without_an_index() {
        let body = json!({"results": [{"relevance_score": 0.5}]});
        assert!(matches!(
            parse_response(&body, 1),
            Err(EmbedError::Malformed(_))
        ));
    }

    #[test]
    fn an_empty_result_list_is_fine() {
        // 服务端认为一条都不相关是合法答复。
        assert!(parse_response(&json!({"results": []}), 3)
            .expect("parse")
            .is_empty());
    }

    #[tokio::test]
    async fn reranking_nothing_is_a_no_op() {
        // 不该为空候选发请求。
        assert!(rerank(&config(), "q", &[], 5)
            .await
            .expect("rerank")
            .is_empty());
        assert!(rerank(&config(), "q", &["a".to_string()], 0)
            .await
            .expect("rerank")
            .is_empty());
    }

    #[tokio::test]
    async fn an_unknown_provider_is_a_config_error() {
        // 配置错不该当成「服务暂时不可用」去重试。
        let mut cfg = config();
        cfg.provider = "ollama".to_string();
        let error = rerank(&cfg, "q", &["a".to_string()], 1)
            .await
            .expect_err("must fail");
        assert!(matches!(error, EmbedError::Config(_)));
        assert!(!error.retryable());
    }

    #[tokio::test]
    async fn a_blank_model_is_a_config_error() {
        let mut cfg = config();
        cfg.model = "  ".to_string();
        assert!(matches!(
            rerank(&cfg, "q", &["a".to_string()], 1).await,
            Err(EmbedError::Config(_))
        ));
    }

    #[tokio::test]
    async fn a_dead_endpoint_is_transient() {
        // 服务没起来是可重试的 —— 调用方会回退到融合名次,下次再试。
        let mut cfg = config();
        cfg.base_url = "http://127.0.0.1:1".to_string();
        let error = rerank(&cfg, "q", &["a".to_string()], 1)
            .await
            .expect_err("must fail");
        assert!(error.retryable(), "got {error:?}");
    }
}
