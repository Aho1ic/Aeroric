//! The models a built-in Codex offers, from its own `model/list` (N4): the
//! catalog lists exactly what the installed Codex says, never a guessed set.
//!
//! The settings catalog cannot reach a process itself; the structured executor
//! installs a [`Fetcher`] and the catalog reads through [`builtin_models`],
//! which caches answers (failures briefly) so a catalog read rarely waits.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::server::CodexServer;
use crate::sessions::live::now_ms;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    /// The slug `turn/start` / `thread/start` take as `model`.
    pub model: String,
    pub efforts: Vec<String>,
    pub is_default: bool,
}

pub type Fetcher = Arc<dyn Fn(&str) -> Result<Vec<CodexModel>, String> + Send + Sync>;

const FRESH_MS: u64 = 10 * 60_000;
const FAILURE_FRESH_MS: u64 = 30_000;
const MAX_PAGES: usize = 10;

type Cache = HashMap<String, (u64, Result<Vec<CodexModel>, String>)>;

fn fetcher() -> &'static Mutex<Option<Fetcher>> {
    static FETCHER: OnceLock<Mutex<Option<Fetcher>>> = OnceLock::new();
    FETCHER.get_or_init(Mutex::default)
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// Whether this process runs structured sessions at all (a host installed a
/// fetcher): the built-in Claude is offered only then (N5).
pub fn native_sessions_enabled() -> bool {
    fetcher().lock().is_some()
}

/// Installed by the host's structured executor; the last one wins.
pub fn install_fetcher(fetch: Fetcher) {
    *fetcher().lock() = Some(fetch);
    cache().lock().clear();
}

/// `None` when no host offers structured Codex (the catalog keeps the built-in
/// entry unavailable); otherwise the cached or freshly fetched answer.
pub fn builtin_models(agent: &str) -> Option<Result<Vec<CodexModel>, String>> {
    let fetch = fetcher().lock().clone()?;
    let now = now_ms();
    if let Some((at, answer)) = cache().lock().get(agent) {
        let fresh = if answer.is_ok() {
            FRESH_MS
        } else {
            FAILURE_FRESH_MS
        };
        if now.saturating_sub(*at) < fresh {
            return Some(answer.clone());
        }
    }
    let answer = fetch(agent);
    cache()
        .lock()
        .insert(agent.to_string(), (now, answer.clone()));
    Some(answer)
}

/// One `model/list` page → models (hidden ones skipped) and the next cursor.
pub fn parse_model_list(result: &Value) -> (Vec<CodexModel>, Option<String>) {
    let models = result
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| !item.get("hidden").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|item| {
                    let model = item.get("model").and_then(Value::as_str)?.to_string();
                    let efforts = item
                        .get("supportedReasoningEfforts")
                        .and_then(Value::as_array)
                        .map(|options| {
                            options
                                .iter()
                                .filter_map(|o| o.get("reasoningEffort").and_then(Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    let is_default = item
                        .get("isDefault")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    Some(CodexModel {
                        model,
                        efforts,
                        is_default,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let next = result
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(str::to_string);
    (models, next)
}

/// Every visible model of the server's Codex, following the cursor.
pub async fn list_models(server: &Arc<CodexServer>) -> Result<Vec<CodexModel>, String> {
    let connection = server.connect().await?;
    let mut models = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let params = match &cursor {
            Some(cursor) => json!({"cursor": cursor}),
            None => json!({}),
        };
        let result = connection
            .client
            .request("model/list", params)
            .await
            .map_err(|e| format!("无法读取 Codex 模型目录：{e}"))?;
        let (page, next) = parse_model_list(&result);
        models.extend(page);
        match next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    if models.is_empty() {
        return Err("Codex 没有返回可用模型；请检查 Codex 登录状态。".into());
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::live::codex::fake::{launched, launcher};

    #[test]
    fn a_page_keeps_visible_models_with_their_efforts() {
        let (models, next) = parse_model_list(&json!({
            "data": [
                {"id": "a", "model": "gpt-5.5", "displayName": "GPT-5.5", "description": "", "hidden": false, "isDefault": true,
                 "defaultReasoningEffort": "medium",
                 "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": ""}, {"reasoningEffort": "high", "description": ""}]},
                {"id": "b", "model": "secret", "displayName": "", "description": "", "hidden": true, "isDefault": false,
                 "defaultReasoningEffort": "low", "supportedReasoningEfforts": []}
            ],
            "nextCursor": "c2"
        }));
        assert_eq!(
            models,
            vec![CodexModel {
                model: "gpt-5.5".into(),
                efforts: vec!["low".into(), "high".into()],
                is_default: true
            }]
        );
        assert_eq!(next.as_deref(), Some("c2"));
    }

    #[tokio::test]
    async fn the_list_follows_the_cursor_through_the_shared_server() {
        let (launcher, mut peers) = launcher();
        let server = CodexServer::new(launcher);
        let listing = tokio::spawn({
            let server = Arc::clone(&server);
            async move { list_models(&server).await }
        });
        let mut peer = launched(&mut peers).await;
        peer.handshake().await;
        let first = peer.expect("model/list").await;
        assert_eq!(first["params"], json!({}));
        peer.reply(
            &first,
            json!({"data": [{"model": "m1", "isDefault": true}], "nextCursor": "p2"}),
        );
        let second = peer.expect("model/list").await;
        assert_eq!(second["params"], json!({"cursor": "p2"}));
        peer.reply(&second, json!({"data": [{"model": "m2"}]}));
        let models = listing.await.unwrap().unwrap();
        assert_eq!(
            models.iter().map(|m| m.model.as_str()).collect::<Vec<_>>(),
            ["m1", "m2"]
        );
    }

    #[tokio::test]
    async fn an_empty_catalog_is_an_error_not_an_empty_choice() {
        let (launcher, mut peers) = launcher();
        let server = CodexServer::new(launcher);
        let listing = tokio::spawn({
            let server = Arc::clone(&server);
            async move { list_models(&server).await }
        });
        let mut peer = launched(&mut peers).await;
        peer.handshake().await;
        let request = peer.expect("model/list").await;
        peer.reply(&request, json!({"data": []}));
        assert!(listing
            .await
            .unwrap()
            .unwrap_err()
            .contains("没有返回可用模型"));
    }
}
