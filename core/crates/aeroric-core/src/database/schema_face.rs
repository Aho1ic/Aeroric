//! P8-05 切片九十八（database 收尾）：schema／驱动面的宿主依赖点。
//!
//! 六条连接面与 `src-tauri/src/database/schema.rs` 同源 verbatim 上收
//! （ensure_connected → dbx-core schema 调用），驱动清单是 include_str 的
//! 静态资产（路径按 core 的四层相对深度换算，与宿主的两层不同）。

use dbx_core::db;
pub use dbx_core::types::ObjectSourceKind;

use crate::database::dbx_connections::{ensure_connected, DbxConnectionPaths};
use crate::database::dbx_secrets::DbxConnectionSecretStore;
use crate::database::dbx_state::DbxState;

const DRIVER_MANIFEST_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../dbx/crates/dbx-core/assets/database-drivers.manifest.json"
));

/// 驱动清单（Tauri drivers.rs `driver_manifest_value` verbatim——只是
/// include_str 的锚点随 crate 换了相对深度）。
pub fn driver_manifest_value() -> Result<serde_json::Value, String> {
    serde_json::from_str(DRIVER_MANIFEST_JSON).map_err(|error| error.to_string())
}

fn required(value: Option<String>, fallback: &str) -> String {
    value.unwrap_or_else(|| fallback.to_string())
}

pub async fn list_databases(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
) -> Result<Vec<db::DatabaseInfo>, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    dbx_core::schema::list_databases_core(&state.app_state, &connection_id).await
}

pub async fn list_schemas(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: Option<String>,
) -> Result<Vec<String>, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    let database = required(database, "");
    dbx_core::schema::list_schemas_core(&state.app_state, &connection_id, &database).await
}

#[allow(clippy::too_many_arguments)]
pub async fn list_objects(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    filter: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    object_types: Option<Vec<String>>,
) -> Result<Vec<db::ObjectInfo>, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
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
        // 新增的 table_name_filter 参数,None 等于不额外按表名过滤。
        None,
    )
    .await
}

pub async fn get_columns(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    table: String,
) -> Result<Vec<db::ColumnInfo>, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
    let database = required(database, "");
    let schema = required(schema, "");
    dbx_core::schema::get_columns_core(&state.app_state, &connection_id, &database, &schema, &table)
        .await
}

pub async fn get_table_ddl(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    table: String,
) -> Result<String, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
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

#[allow(clippy::too_many_arguments)]
pub async fn get_object_source(
    state: &DbxState,
    paths: &DbxConnectionPaths,
    store: &dyn DbxConnectionSecretStore,
    connection_id: String,
    database: Option<String>,
    schema: Option<String>,
    name: String,
    object_type: db::ObjectSourceKind,
    signature: Option<String>,
) -> Result<db::ObjectSource, String> {
    ensure_connected(state, paths, store, &connection_id).await?;
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
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::driver_manifest_value;

    #[test]
    fn dbx_driver_manifest_contains_core_drivers() {
        let manifest = driver_manifest_value().unwrap();
        let drivers = manifest["drivers"].as_array().unwrap();
        let driver_types = drivers
            .iter()
            .filter_map(|driver| driver["dbType"].as_str())
            .collect::<Vec<_>>();

        assert!(driver_types.contains(&"sqlite"));
        assert!(driver_types.contains(&"postgres"));
        assert!(driver_types.contains(&"mysql"));
        assert!(driver_types.contains(&"redis"));
        assert!(driver_types.contains(&"mongodb"));
    }
}
