use std::future::Future;
use std::sync::Arc;

use dbx_core::connection::AppState;
use dbx_core::db::mongo_driver::MongoDocumentResult;
use tauri::State;

use super::connections;
use super::dbx_state::DbxState;

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

#[tauri::command]
pub async fn dbx_mongo_list_databases(
    state: State<'_, DbxState>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::mongo_ops::mongo_list_databases_core(&state.app_state, &connection_id).await
}

#[tauri::command]
pub async fn dbx_mongo_list_collections(
    state: State<'_, DbxState>,
    connection_id: String,
    database: String,
) -> Result<Vec<String>, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::mongo_ops::mongo_list_collections_core(&state.app_state, &connection_id, &database)
        .await
        .map(|collections| {
            collections
                .into_iter()
                .map(|collection| collection.name)
                .collect()
        })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn dbx_mongo_find_documents(
    state: State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    skip: Option<u64>,
    limit: Option<i64>,
    filter: Option<String>,
    sort: Option<String>,
    execution_id: Option<String>,
) -> Result<MongoDocumentResult, String> {
    connections::ensure_connected(&state, &connection_id).await?;
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
            sort.as_deref(),
        ),
    )
    .await
}

#[tauri::command]
pub async fn dbx_mongo_insert_document(
    state: State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    doc_json: String,
) -> Result<String, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "Insert").await?;
    dbx_core::mongo_ops::mongo_insert_document_core(
        &state.app_state,
        &connection_id,
        &database,
        &collection,
        &doc_json,
    )
    .await
}

#[tauri::command]
pub async fn dbx_mongo_update_document(
    state: State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    id: String,
    doc_json: String,
) -> Result<u64, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "Update").await?;
    dbx_core::mongo_ops::mongo_update_document_core(
        &state.app_state,
        &connection_id,
        &database,
        &collection,
        &id,
        &doc_json,
    )
    .await
}

#[tauri::command]
pub async fn dbx_mongo_delete_documents(
    state: State<'_, DbxState>,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    many: Option<bool>,
) -> Result<u64, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    connections::ensure_writable(&state, &connection_id, "Delete").await?;
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
