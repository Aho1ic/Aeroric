use dbx_core::db;
use tauri::State;

use super::connections;
use super::dbx_state::DbxState;

fn required(value: Option<String>, fallback: &str) -> String {
    value.unwrap_or_else(|| fallback.to_string())
}

#[tauri::command]
pub async fn dbx_list_databases(
    state: State<'_, DbxState>,
    connection_id: String,
) -> Result<Vec<db::DatabaseInfo>, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    dbx_core::schema::list_databases_core(&state.app_state, &connection_id).await
}

#[tauri::command]
pub async fn dbx_list_schemas(
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
) -> Result<Vec<String>, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    let database = required(database, "");
    dbx_core::schema::list_schemas_core(&state.app_state, &connection_id, &database).await
}

#[tauri::command]
pub async fn dbx_list_objects(
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    filter: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    object_types: Option<Vec<String>>,
) -> Result<Vec<db::ObjectInfo>, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    let database = required(database, "");
    let schema = required(schema, "");
    dbx_core::schema::list_objects_core(
        &state.app_state,
        &connection_id,
        &database,
        &schema,
        filter.as_deref(),
        limit,
        offset,
        object_types.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn dbx_get_columns(
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    table: String,
) -> Result<Vec<db::ColumnInfo>, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    let database = required(database, "");
    let schema = required(schema, "");
    dbx_core::schema::get_columns_core(&state.app_state, &connection_id, &database, &schema, &table)
        .await
}

#[tauri::command]
pub async fn dbx_get_table_ddl(
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    table: String,
) -> Result<String, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    let database = required(database, "");
    let schema = required(schema, "");
    dbx_core::schema::get_table_ddl_core(
        &state.app_state,
        &connection_id,
        &database,
        &schema,
        &table,
        None,
    )
    .await
}

#[tauri::command]
pub async fn dbx_get_object_source(
    state: State<'_, DbxState>,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    name: String,
    object_type: db::ObjectSourceKind,
    signature: Option<String>,
) -> Result<db::ObjectSource, String> {
    connections::ensure_connected(&state, &connection_id).await?;
    let database = required(database, "");
    let schema = required(schema, "");
    dbx_core::schema::get_object_source_core(
        &state.app_state,
        &connection_id,
        &database,
        &schema,
        &name,
        object_type,
        signature.as_deref(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::required;

    #[test]
    fn required_uses_fallback_for_missing_scope() {
        assert_eq!(required(None, ""), "");
        assert_eq!(required(Some("public".to_string()), ""), "public");
    }
}
