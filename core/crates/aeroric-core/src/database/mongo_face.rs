//! P8-05 切片一百零一（database）：mongo 面 6 条的宿主依赖点。与
//! `src-tauri/src/database/mongo.rs` 同源 verbatim 上收：run_cancellable 的
//! 取消登记、可写闸、dbx-core mongo_ops 调用逐字。

use std::future::Future;
use std::sync::Arc;

use dbx_core::connection::AppState;
use dbx_core::db::mongo_driver::MongoDocumentResult;

use crate::database::dbx_connections::{ensure_connected, ensure_writable, DbxConnectionPaths};
use crate::database::dbx_secrets::DbxConnectionSecretStore;
use crate::database::dbx_state::DbxState;

async fn run_cancellable<T, F>(
    state: &Arc<AppState>,
    execution_id: Option<String>,
    future: F,
) -> Result<T, String>
where
    F: Future<Output = Result<T, String>>,
{
    let registered_query = execution_id
        .as_ref()
        .and_then(|id| non_empty(id.clone()))
        .map(|id| state.running_queries.register(id));
    if let Some(query) = registered_query.as_ref() {
        let token = query.token();
        tokio::select! {
            biased;
            _ = token.cancelled() => Err(dbx_core::query::canceled_error()),
            result = future => result,
        }
    } else {
        future.await
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(100).clamp(1, 1000)
}

pub async fn dbx_mongo_list_databases(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
) -> Result<Vec<String>, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    dbx_core::mongo_ops::mongo_list_databases_core(&state.app_state, &connection_id).await
}

pub async fn dbx_mongo_list_collections(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: String,
) -> Result<Vec<String>, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    dbx_core::mongo_ops::mongo_list_collections_core(&state.app_state, &connection_id, &database)
        .await
        .map(|collections| {
            collections
                .into_iter()
                .map(|collection| collection.name)
                .collect()
        })
}

#[allow(clippy::too_many_arguments)]
pub async fn dbx_mongo_find_documents(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: String,
    collection: String,
    skip: Option<u64>,
    limit: Option<i64>,
    filter: Option<String>,
    projection: Option<String>,
    sort: Option<String>,
    execution_id: Option<String>,
) -> Result<MongoDocumentResult, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    let app = state.app_state.clone();
    run_cancellable(
        &app,
        execution_id,
        dbx_core::mongo_ops::mongo_find_documents_core(
            &app,
            &connection_id,
            &database,
            &collection,
            skip.unwrap_or(0),
            normalize_limit(limit),
            filter.as_deref(),
            projection.as_deref(),
            sort.as_deref(),
            // 新增的 collation 参数,None 等于原来的默认排序规则。
            None,
        ),
    )
    .await
}

pub async fn dbx_mongo_insert_document(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: String,
    collection: String,
    doc_json: String,
) -> Result<String, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    ensure_writable(state, &connection_id, "Insert").await?;
    dbx_core::mongo_ops::mongo_insert_document_core(
        &state.app_state,
        &connection_id,
        &database,
        &collection,
        &doc_json,
    )
    .await
}

pub async fn dbx_mongo_update_document(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: String,
    collection: String,
    id: String,
    doc_json: String,
    routing: Option<String>,
) -> Result<u64, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    ensure_writable(state, &connection_id, "Update").await?;
    dbx_core::mongo_ops::mongo_update_document_core(
        &state.app_state,
        &connection_id,
        &database,
        &collection,
        &id,
        &doc_json,
        routing.as_deref(),
    )
    .await
}

pub async fn dbx_mongo_delete_documents(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    many: Option<bool>,
) -> Result<u64, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    ensure_writable(state, &connection_id, "Delete").await?;
    dbx_core::mongo_ops::mongo_delete_documents_core(
        &state.app_state,
        &connection_id,
        &database,
        &collection,
        &filter_json,
        many.unwrap_or(false),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{non_empty, normalize_limit};

    #[test]
    fn mongo_defaults_are_bounded() {
        assert_eq!(non_empty(" exec ".to_string()), Some("exec".to_string()));
        assert_eq!(non_empty("  ".to_string()), None);
        assert_eq!(normalize_limit(None), 100);
        assert_eq!(normalize_limit(Some(0)), 1);
        assert_eq!(normalize_limit(Some(5000)), 1000);
    }
}
