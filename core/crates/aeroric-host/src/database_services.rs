//! P8-05 切片九十四（database 刀零）：legacy (v1) SQLite 文件命令的宿主服务面。
//!
//! 八条 `database.*` 方法；业务主体全部在 core `database::legacy_sqlite`，
//! 这里只做四件事：把 wire 参数翻成 core 入参、把连接文件锚在宿主 data-dir、
//! 做 saved 连接的权威只读判定（Value 直读直写——typed round-trip 会丢未知
//! 字段，字节保真纪律 favor 保留）、把 SSH endpoint 显式拒掉（宿主没有远程
//! 数据库执行器——下载/回传那半边留在 Tauri 适配层）。连接文件与 Tauri 共用
//! 同一个 `<data-dir>/database-connections.json`（单写者租约保证两个进程不会
//! 同时持有它），读写都按 legacy 的私有权限语义。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::database::dbx_connections as dbx;
use aeroric_core::database::dbx_state::DbxState;
use aeroric_core::database::execution;
use aeroric_core::database::grid as grid_biz;
use aeroric_core::database::legacy_sqlite as biz;
use aeroric_core::database::schema_face;
use aeroric_core::database::sql_builders as builders;
use aeroric_core::events::EventLog;
use aeroric_core::maintenance::StartupDegradationLog;
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

const CONNECTIONS_FILE: &str = "database-connections.json";
const DBX_APP_VERSION: &str = concat!("aeroric-host ", env!("CARGO_PKG_VERSION"));

/// Per service assembly, never process-global: distinct hosts/data dirs must
/// not share pools, connection registries or lazy initialization.
struct HostedDbx {
    state: tokio::sync::OnceCell<DbxState>,
    operations: tokio::sync::Mutex<()>,
    degradations: Arc<StartupDegradationLog>,
}

impl HostedDbx {
    async fn state(&self, context: &CoreContext) -> &DbxState {
        self.state
            .get_or_init(|| async {
                let (state, degradation) =
                    DbxState::new(context.paths.data_dir(), DBX_APP_VERSION).await;
                if let Some(reason) = degradation {
                    self.degradations.record("dbx-state", reason, ":memory:");
                }
                state
            })
            .await
    }
}

/// 切片九十八：schema／驱动面统一 decode→call→reply（连接面 async 直接
/// await；driver manifest 是静态资产，无需连接）。
async fn schema_dispatch(
    request: Request,
    context: Arc<CoreContext>,
    _log: Arc<EventLog>,
    hosted: Arc<HostedDbx>,
    method: &str,
) -> Frame {
    if !request.params.is_object() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params object",
        );
    }
    let params = request.params.clone();
    let paths = dbx_paths(&context);
    let store = dbx_store(&context);
    let id = request.id.clone();
    let opt_string = |name: &str| params.get(name).and_then(Value::as_str).map(str::to_string);
    let state = hosted.state(&context).await;
    match method {
        "database.dbx_list_databases" => {
            let Some(connection_id) = str_param(&params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            match schema_face::list_databases(state, &paths, &store, connection_id).await {
                Ok(result) => typed_reply(&request, &result),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        "database.dbx_list_schemas" => {
            let Some(connection_id) = str_param(&params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            match schema_face::list_schemas(
                state,
                &paths,
                &store,
                connection_id,
                opt_string("database"),
            )
            .await
            {
                Ok(result) => typed_reply(&request, &result),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        "database.dbx_list_objects" => {
            let Some(connection_id) = str_param(&params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            let object_types = params
                .get("objectTypes")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                });
            match schema_face::list_objects(
                state,
                &paths,
                &store,
                connection_id,
                opt_string("database"),
                opt_string("schema"),
                opt_string("filter"),
                params
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize),
                params
                    .get("offset")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize),
                object_types,
            )
            .await
            {
                Ok(result) => typed_reply(&request, &result),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        "database.dbx_get_columns" => {
            let Some(connection_id) = str_param(&params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            let Some(table) = str_param(&params, "table") else {
                return invalid(&request, "table (string)");
            };
            match schema_face::get_columns(
                state,
                &paths,
                &store,
                connection_id,
                opt_string("database"),
                opt_string("schema"),
                table,
            )
            .await
            {
                Ok(result) => typed_reply(&request, &result),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        "database.dbx_get_table_ddl" => {
            let Some(connection_id) = str_param(&params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            let Some(table) = str_param(&params, "table") else {
                return invalid(&request, "table (string)");
            };
            match schema_face::get_table_ddl(
                state,
                &paths,
                &store,
                connection_id,
                opt_string("database"),
                opt_string("schema"),
                table,
            )
            .await
            {
                Ok(result) => typed_reply(&request, &result),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        "database.dbx_get_object_source" => {
            let Some(connection_id) = str_param(&params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            let Some(name) = str_param(&params, "name") else {
                return invalid(&request, "name (string)");
            };
            let Some(object_type) = params.get("objectType").and_then(Value::as_str) else {
                return invalid(&request, "objectType (string)");
            };
            let Ok(kind) = serde_json::from_value::<
                aeroric_core::database::schema_face::ObjectSourceKind,
            >(Value::String(object_type.to_string())) else {
                return invalid(&request, "objectType (known object source kind)");
            };
            match schema_face::get_object_source(
                state,
                &paths,
                &store,
                connection_id,
                opt_string("database"),
                opt_string("schema"),
                name,
                kind,
                opt_string("signature"),
            )
            .await
            {
                Ok(result) => typed_reply(&request, &result),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        _ => failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("unknown database schema method {method}"),
        ),
    }
}

/// 切片九十七：执行面统一 decode→call→reply。连接池/注册表是 async 业务，
/// 直接 await（spawn_blocking 拿不到运行时——dbx 五条同款注释）。
async fn execution_dispatch(
    request: Request,
    context: Arc<CoreContext>,
    log: Arc<EventLog>,
    hosted: Arc<HostedDbx>,
    method: &str,
) -> Frame {
    if !request.params.is_object() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params object",
        );
    }
    let params = request.params.clone();
    let paths = dbx_paths(&context);
    let store = dbx_store(&context);
    fn decode_request<T: serde::de::DeserializeOwned>(
        request: &Request,
        params: &Value,
    ) -> Result<T, Box<Frame>> {
        let body = params.get("request").cloned().unwrap_or(Value::Null);
        serde_json::from_value(body)
            .map_err(|error| Box::new(invalid(request, &format!("request: {error}"))))
    }

    let sink = confirm_event_sink(&log);
    let id = request.id.clone();
    match method {
        "database.dbx_execute_query" => {
            match decode_request::<execution::ExecuteQueryRequest>(&request, &params) {
                Ok(parsed) => {
                    match execution::execute_query(
                        hosted.state(&context).await,
                        &paths,
                        &store,
                        Some(&sink),
                        parsed,
                    )
                    .await
                    {
                        Ok(result) => typed_reply(&request, &result),
                        Err(error) => failure(id, ErrorCode::IoError, &error),
                    }
                }
                Err(frame) => *frame,
            }
        }
        "database.dbx_execute_multi" => {
            match decode_request::<execution::ExecuteMultiRequest>(&request, &params) {
                Ok(parsed) => {
                    match execution::execute_multi(
                        hosted.state(&context).await,
                        &paths,
                        &store,
                        Some(&sink),
                        parsed,
                    )
                    .await
                    {
                        Ok(result) => typed_reply(&request, &result),
                        Err(error) => failure(id, ErrorCode::IoError, &error),
                    }
                }
                Err(frame) => *frame,
            }
        }
        "database.dbx_cancel_query" => {
            let Some(execution_id) = str_param(&params, "executionId") else {
                return invalid(&request, "executionId (string)");
            };
            match execution::cancel_query(hosted.state(&context).await, execution_id) {
                Ok(()) => ok(&request, Value::Null),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        "database.dbx_close_result_session" => {
            let Some(connection_id) = str_param(&params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            let Some(session_id) = str_param(&params, "sessionId") else {
                return invalid(&request, "sessionId (string)");
            };
            let database = params
                .get("database")
                .and_then(Value::as_str)
                .map(str::to_string);
            let client_session_id = params
                .get("clientSessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
            match execution::close_result_session(
                hosted.state(&context).await,
                connection_id,
                session_id,
                database,
                client_session_id,
            )
            .await
            {
                Ok(()) => ok(&request, Value::Null),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        "database.dbx_query_table_data" => {
            match decode_request::<execution::GridTableDataRequest>(&request, &params) {
                Ok(parsed) => {
                    match execution::query_table_data(
                        hosted.state(&context).await,
                        &paths,
                        &store,
                        parsed,
                    )
                    .await
                    {
                        Ok(result) => ok(&request, result),
                        Err(error) => failure(id, ErrorCode::IoError, &error),
                    }
                }
                Err(frame) => *frame,
            }
        }
        "database.dbx_update_cell" | "database.dbx_insert_row" | "database.dbx_delete_rows" => {
            match decode_request::<execution::GridSaveRequest>(&request, &params) {
                Ok(parsed) => {
                    match execution::execute_grid_save(
                        hosted.state(&context).await,
                        &paths,
                        &store,
                        parsed,
                    )
                    .await
                    {
                        Ok(result) => ok(&request, result),
                        Err(error) => failure(id, ErrorCode::IoError, &error),
                    }
                }
                Err(frame) => *frame,
            }
        }
        "database.dbx_assess_production_sql" => {
            match decode_request::<execution::AssessProductionSqlRequest>(&request, &params) {
                Ok(parsed) => {
                    match execution::assess_sql(
                        hosted.state(&context).await,
                        &paths,
                        &store,
                        parsed,
                    )
                    .await
                    {
                        Ok(result) => typed_reply(&request, &result),
                        Err(error) => failure(id, ErrorCode::IoError, &error),
                    }
                }
                Err(frame) => *frame,
            }
        }
        "database.dbx_assess_production_target" => {
            match decode_request::<execution::AssessProductionTargetRequest>(&request, &params) {
                Ok(parsed) => {
                    match execution::assess_target(
                        hosted.state(&context).await,
                        &paths,
                        &store,
                        parsed,
                    )
                    .await
                    {
                        Ok(result) => typed_reply(&request, &result),
                        Err(error) => failure(id, ErrorCode::IoError, &error),
                    }
                }
                Err(frame) => *frame,
            }
        }
        "database.respond_dbx_production_confirmation" => {
            let Some(request_id) = str_param(&params, "requestId") else {
                return invalid(&request, "requestId (string)");
            };
            let Some(approved) = params.get("approved").and_then(Value::as_bool) else {
                return invalid(&request, "approved (boolean)");
            };
            let mut pending = hosted
                .state(&context)
                .await
                .pending_production_confirmations
                .lock()
                .await;
            match execution::resolve_pending_production_confirmation(
                &mut pending,
                &request_id,
                approved,
            ) {
                Ok(()) => ok(&request, Value::Null),
                Err(error) => failure(id, ErrorCode::IoError, &error),
            }
        }
        _ => failure(
            id,
            ErrorCode::InvalidArgument,
            &format!("unknown database execution method {method}"),
        ),
    }
}

fn typed_reply<T: serde::Serialize>(request: &Request, value: &T) -> Frame {
    match serde_json::to_value(value) {
        Ok(value) => ok(request, value),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error.to_string()),
    }
}

/// 确认请求出口：EventLog append（legacy 通道名，v1 台账映射
/// `dbx.production_confirm_requested`）。core 不认识连接——出口注入。
fn confirm_event_sink(log: &Arc<EventLog>) -> execution::ConfirmEventSink {
    let log = Arc::clone(log);
    Arc::new(move |request: &execution::ProductionConfirmRequest| {
        if let Ok(payload) = serde_json::to_value(request) {
            log.append(execution::PRODUCTION_CONFIRM_EVENT, payload);
        }
    })
}

fn dbx_paths(context: &CoreContext) -> dbx::DbxConnectionPaths {
    dbx::DbxConnectionPaths::from_data_dir(context.paths.data_dir())
}

/// 切片九十六：SQL builders／grid 预览的统一 decode→call→reply。
/// 纯函数（无状态、无 IO、无 await），失败面只有两种：`options` 解不开
/// （InvalidArgument）与 builder 自身校验拒绝（IoError，legacy Tauri 命令的
/// Err(String) 同文案——见 with_local 的同款映射）。
fn builder_dispatch(request: Request, method: &str) -> Frame {
    let Some(params) = request.params.as_object() else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &format!("{method} requires params object"),
        );
    };
    let options = params.get("options").cloned().unwrap_or(Value::Null);

    macro_rules! build {
        ($options_ty:ty, $call:expr) => {
            match serde_json::from_value::<$options_ty>(options) {
                Ok(options) => match serde_json::to_value(($call)(options)) {
                    Ok(value) => ok(&request, value),
                    Err(error) => {
                        failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
                    }
                },
                Err(error) => invalid(&request, &format!("options: {error}")),
            }
        };
    }
    macro_rules! build_checked {
        ($options_ty:ty, $call:expr) => {
            match serde_json::from_value::<$options_ty>(options) {
                Ok(options) => match ($call)(options) {
                    Ok(value) => match serde_json::to_value(value) {
                        Ok(value) => ok(&request, value),
                        Err(error) => {
                            failure(request.id.clone(), ErrorCode::IoError, &error.to_string())
                        }
                    },
                    Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                },
                Err(error) => invalid(&request, &format!("options: {error}")),
            }
        };
    }

    match method {
        "database.dbx_build_table_structure_change_sql" => build!(
            builders::TableStructureSqlOptions,
            builders::build_table_structure_change_sql
        ),
        "database.dbx_build_single_column_alter_sql" => build!(
            builders::SingleColumnAlterSqlOptions,
            builders::build_single_column_alter_sql
        ),
        "database.dbx_build_create_database_sql" => build_checked!(
            builders::CreateDatabaseSqlOptions,
            builders::build_create_database_sql
        ),
        "database.dbx_build_duckdb_attach_database_sql" => build!(
            builders::DuckDbAttachDatabaseSqlOptions,
            builders::build_duckdb_attach_database_sql
        ),
        "database.dbx_build_rename_object_sql" => build_checked!(
            builders::RenameObjectSqlOptions,
            builders::build_rename_object_sql
        ),
        "database.dbx_build_drop_database_sql" => build!(
            builders::DatabaseNameSqlOptions,
            builders::build_drop_database_sql
        ),
        "database.dbx_build_create_schema_sql" => build_checked!(
            builders::SchemaNameSqlOptions,
            builders::build_create_schema_sql
        ),
        "database.dbx_build_drop_schema_sql" => build!(
            builders::SchemaNameSqlOptions,
            builders::build_drop_schema_sql
        ),
        "database.dbx_build_drop_table_sql" => build!(
            builders::TableAdminSqlOptions,
            builders::build_drop_table_sql
        ),
        "database.dbx_build_truncate_table_sql" => build!(
            builders::TableAdminSqlOptions,
            builders::build_truncate_table_sql
        ),
        "database.dbx_build_empty_table_sql" => build!(
            builders::TableAdminSqlOptions,
            builders::build_empty_table_sql
        ),
        "database.dbx_build_drop_object_sql" => build!(
            builders::DropObjectSqlOptions,
            builders::build_drop_object_sql
        ),
        "database.dbx_build_drop_table_child_object_sql" => build_checked!(
            builders::DropTableChildObjectSqlOptions,
            builders::build_drop_table_child_object_sql
        ),
        "database.dbx_build_duplicate_table_structure_sql" => build!(
            builders::DuplicateTableStructureSqlOptions,
            builders::build_duplicate_table_structure_sql
        ),
        "database.dbx_build_database_search_sql" => build!(
            builders::DatabaseSearchSqlOptions,
            builders::build_database_search_sql
        ),
        "database.dbx_build_search_result_where" => build!(
            builders::SearchResultWhereOptions,
            builders::build_search_result_where
        ),
        "database.dbx_preview_grid_sql" => {
            build!(
                grid_biz::DataGridSaveStatementOptions,
                grid_biz::preview_grid_sql
            )
        }
        "database.dbx_build_data_grid_copy_insert_statement" => build!(
            grid_biz::DataGridCopyInsertStatementOptions,
            grid_biz::build_data_grid_copy_insert_statement
        ),
        "database.dbx_build_data_grid_copy_update_statements" => build!(
            grid_biz::DataGridCopyUpdateStatementOptions,
            grid_biz::build_data_grid_copy_update_statements
        ),
        "database.dbx_build_data_grid_context_filter_condition" => build!(
            grid_biz::DataGridContextFilterConditionOptions,
            grid_biz::build_data_grid_context_filter_condition
        ),
        _ => failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &format!("unknown database builder method {method}"),
        ),
    }
}

fn dbx_store(
    context: &CoreContext,
) -> aeroric_core::database::dbx_secrets::SecretStoreDbxSecretStore<'_> {
    aeroric_core::database::dbx_secrets::SecretStoreDbxSecretStore {
        store: context.secrets.as_ref(),
    }
}

/// 注册点逐条写成源码字面量：本仓的覆盖率/台账扫描器靠 `set.register("…"`
/// 的字面量找注册，循环＋变量会变成覆盖率假缺口（切片九的同款陷阱）。
pub fn register_database_services(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    degradations: Arc<StartupDegradationLog>,
    log: Arc<EventLog>,
) {
    let dbx = Arc::new(HostedDbx {
        state: tokio::sync::OnceCell::new(),
        operations: tokio::sync::Mutex::new(()),
        degradations,
    });
    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_inspect",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_inspect";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_query_table",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_query_table";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_execute_sql",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_execute_sql";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_update_cell",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_update_cell";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_insert_row",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_insert_row";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_delete_row",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_delete_row";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_load_connections",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_load_connections";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    {
        // dbx 五条的命令体是 tokio RwLock 的 async 业务（连接池注册在 async
        // 上下文），直接 await dispatch；spawn_blocking 里拿不到运行时。
        let context = Arc::clone(&context);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_list_connections",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move { dbx_dispatch(request, context, dbx).await })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_save_connection",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move { dbx_dispatch(request, context, dbx).await })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_delete_connection",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move { dbx_dispatch(request, context, dbx).await })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_test_connection",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move { dbx_dispatch(request, context, dbx).await })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_connect",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move { dbx_dispatch(request, context, dbx).await })
            }),
        );
    }
    // ── 切片九十七（database 刀三）：执行面＋生产确认回路 11 条 ──
    // 确认请求出口＝EventLog append（legacy 通道名；executor "task-status"
    // 先例）——core 不认识连接，emit 出口是注入的 ConfirmEventSink。
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_execute_query",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(request, context, log, dbx, "database.dbx_execute_query")
                        .await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_execute_multi",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(request, context, log, dbx, "database.dbx_execute_multi")
                        .await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_cancel_query",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(request, context, log, dbx, "database.dbx_cancel_query")
                        .await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_close_result_session",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(
                        request,
                        context,
                        log,
                        dbx,
                        "database.dbx_close_result_session",
                    )
                    .await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_query_table_data",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(request, context, log, dbx, "database.dbx_query_table_data")
                        .await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_update_cell",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(request, context, log, dbx, "database.dbx_update_cell").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_insert_row",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(request, context, log, dbx, "database.dbx_insert_row").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_delete_rows",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(request, context, log, dbx, "database.dbx_delete_rows").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_assess_production_sql",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(
                        request,
                        context,
                        log,
                        dbx,
                        "database.dbx_assess_production_sql",
                    )
                    .await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_assess_production_target",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(
                        request,
                        context,
                        log,
                        dbx,
                        "database.dbx_assess_production_target",
                    )
                    .await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.respond_dbx_production_confirmation",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    execution_dispatch(
                        request,
                        context,
                        log,
                        dbx,
                        "database.respond_dbx_production_confirmation",
                    )
                    .await
                })
            }),
        );
    }
    // ── 切片九十八（database 收尾）：schema／驱动面 7 条 ──
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_list_databases",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    schema_dispatch(request, context, log, dbx, "database.dbx_list_databases").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_list_schemas",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    schema_dispatch(request, context, log, dbx, "database.dbx_list_schemas").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_list_objects",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    schema_dispatch(request, context, log, dbx, "database.dbx_list_objects").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_get_columns",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    schema_dispatch(request, context, log, dbx, "database.dbx_get_columns").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_get_table_ddl",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    schema_dispatch(request, context, log, dbx, "database.dbx_get_table_ddl").await
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let log = Arc::clone(&log);
        let dbx = Arc::clone(&dbx);
        set.register(
            "database.dbx_get_object_source",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let log = Arc::clone(&log);
                let dbx = Arc::clone(&dbx);
                Box::pin(async move {
                    schema_dispatch(request, context, log, dbx, "database.dbx_get_object_source")
                        .await
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_driver_manifest",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    match schema_face::driver_manifest_value() {
                        Ok(value) => ok(&request, value),
                        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
                    }
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        set.register(
            "database.db_save_connections",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let method = "database.db_save_connections";
                Box::pin(async move {
                    let id = request.id.clone();
                    let run = move || dispatch(request, context, method);
                    match tokio::task::spawn_blocking(run).await {
                        Ok(frame) => frame,
                        Err(error) => failure(id, ErrorCode::IoError, &error.to_string()),
                    }
                })
            }),
        );
    }

    // ── 切片九十六（database 刀二）：SQL builders／grid 预览纯函数族 20 条 ──
    // 注册点逐条写成源码字面量（覆盖率/台账扫描器只认 `set.register("…"` 的
    // 字面量——切片九同款陷阱第四次规避）。纯函数无状态、无 IO、不需要
    // context；decode `options` → core 再导出函数 → typed reply。
    {
        set.register(
            "database.dbx_build_table_structure_change_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_table_structure_change_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_single_column_alter_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_single_column_alter_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_create_database_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_create_database_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_duckdb_attach_database_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_duckdb_attach_database_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_rename_object_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_rename_object_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_drop_database_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_drop_database_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_create_schema_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_create_schema_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_drop_schema_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(
                    async move { builder_dispatch(request, "database.dbx_build_drop_schema_sql") },
                )
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_drop_table_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(
                    async move { builder_dispatch(request, "database.dbx_build_drop_table_sql") },
                )
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_truncate_table_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_truncate_table_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_empty_table_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(
                    async move { builder_dispatch(request, "database.dbx_build_empty_table_sql") },
                )
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_drop_object_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(
                    async move { builder_dispatch(request, "database.dbx_build_drop_object_sql") },
                )
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_drop_table_child_object_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_drop_table_child_object_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_duplicate_table_structure_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_duplicate_table_structure_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_database_search_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_database_search_sql")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_search_result_where",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(request, "database.dbx_build_search_result_where")
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_preview_grid_sql",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move { builder_dispatch(request, "database.dbx_preview_grid_sql") })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_data_grid_copy_insert_statement",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(
                        request,
                        "database.dbx_build_data_grid_copy_insert_statement",
                    )
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_data_grid_copy_update_statements",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(
                        request,
                        "database.dbx_build_data_grid_copy_update_statements",
                    )
                })
            }),
        );
    }
    {
        set.register(
            "database.dbx_build_data_grid_context_filter_condition",
            Arc::new(move |request: Request, _call| {
                Box::pin(async move {
                    builder_dispatch(
                        request,
                        "database.dbx_build_data_grid_context_filter_condition",
                    )
                })
            }),
        );
    }
}

fn dispatch(request: Request, context: Arc<CoreContext>, method: &str) -> Frame {
    let Some(params) = request.params.as_object() else {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &format!("{method} requires params object"),
        );
    };
    let params = Value::Object(params.clone());
    let connections_path = context.paths.data_dir().join(CONNECTIONS_FILE);
    match method {
        "database.db_inspect" => {
            let endpoint = param_value(&params, "endpoint");
            let project_root = match opt_string(&request, &params, "projectRoot") {
                Ok(value) => value,
                Err(frame) => return *frame,
            };
            with_local(&request, endpoint, project_root, |path| {
                let schema: Value = biz::inspect_schema(path).and_then(|schema| {
                    serde_json::to_value(&schema).map_err(|error| error.to_string())
                })?;
                Ok(schema)
            })
        }
        "database.db_query_table" => {
            let Some(table) = str_param(&params, "table") else {
                return invalid(&request, "table (string)");
            };
            let Some(page) = int_param(&params, "page") else {
                return invalid(&request, "page (integer)");
            };
            let page_size = params.get("pageSize").and_then(Value::as_i64);
            let project_root = match opt_string(&request, &params, "projectRoot") {
                Ok(value) => value,
                Err(frame) => return *frame,
            };
            let endpoint = param_value(&params, "endpoint");
            with_local(&request, endpoint, project_root, move |path| {
                biz::query_table(path, &table, page, biz::normalize_page_size(page_size))
                    .and_then(|result| serde_json::to_value(&result).map_err(|e| e.to_string()))
            })
        }
        "database.db_execute_sql" => {
            let Some(sql) = str_param(&params, "sql") else {
                return invalid(&request, "sql (string)");
            };
            let page = params.get("page").and_then(Value::as_i64);
            let page_size = params.get("pageSize").and_then(Value::as_i64);
            let read_only = match effective_read_only(&params, &connections_path) {
                Ok(read_only) => read_only,
                Err(message) => return failure(request.id.clone(), ErrorCode::IoError, &message),
            };
            let project_root = match opt_string(&request, &params, "projectRoot") {
                Ok(value) => value,
                Err(frame) => return *frame,
            };
            let endpoint = param_value(&params, "endpoint");
            with_local(&request, endpoint, project_root, move |path| {
                biz::execute_sql(path, &sql, page.unwrap_or(1), page_size, read_only)
                    .and_then(|result| serde_json::to_value(&result).map_err(|e| e.to_string()))
            })
        }
        "database.db_update_cell" | "database.db_insert_row" | "database.db_delete_row" => {
            let Some(table) = str_param(&params, "table") else {
                return invalid(&request, "table (string)");
            };
            let read_only = match effective_read_only(&params, &connections_path) {
                Ok(read_only) => read_only,
                Err(message) => return failure(request.id.clone(), ErrorCode::IoError, &message),
            };
            let project_root = match opt_string(&request, &params, "projectRoot") {
                Ok(value) => value,
                Err(frame) => return *frame,
            };
            let endpoint = param_value(&params, "endpoint");
            let row_key_value = param_value(&params, "rowKey");
            let values_value = param_value(&params, "values");
            let column = params
                .get("column")
                .and_then(Value::as_str)
                .map(str::to_string);
            let value_string = params
                .get("value")
                .and_then(Value::as_str)
                .map(str::to_string);
            let is_update = method == "database.db_update_cell";
            let is_insert = method == "database.db_insert_row";
            with_local(&request, endpoint, project_root, move |path| {
                if is_update {
                    let Some(column) = column else {
                        return Err("requires params.column (string)".to_string());
                    };
                    let row_key: biz::DbRowKey = serde_json::from_value(row_key_value)
                        .map_err(|error| format!("rowKey: {error}"))?;
                    biz::update_cell(path, &table, row_key, &column, value_string, read_only)?;
                    Ok(Value::Null)
                } else if is_insert {
                    let values: Vec<biz::DbCellValue> = serde_json::from_value(values_value)
                        .map_err(|error| format!("values: {error}"))?;
                    biz::insert_row(path, &table, values, read_only)?;
                    Ok(Value::Null)
                } else {
                    let row_key: biz::DbRowKey = serde_json::from_value(row_key_value)
                        .map_err(|error| format!("rowKey: {error}"))?;
                    biz::delete_row(path, &table, row_key, read_only)?;
                    Ok(Value::Null)
                }
            })
        }
        "database.db_load_connections" => match load_connections(&connections_path) {
            Ok(value) => ok(&request, value),
            Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
        },
        "database.db_save_connections" => {
            let Some(connections) = params.get("connections") else {
                return invalid(&request, "connections (array)");
            };
            if let Err(message) = validate_connections_shape(connections) {
                return invalid(&request, &message);
            }
            match save_connections(&connections_path, connections) {
                Ok(()) => ok(&request, Value::Null),
                Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
            }
        }
        _ => failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &format!("unknown database method {method}"),
        ),
    }
}

/// Decode wire arguments without IO; initialization itself needs the writer
/// because list can create storage, chmod JSON and migrate credentials on read.
async fn dbx_dispatch(
    request: Request,
    context: Arc<CoreContext>,
    hosted: Arc<HostedDbx>,
) -> Frame {
    use aeroric_core::database::dbx_types::AeroricDbConnectionConfig;

    enum Operation {
        List,
        Save(AeroricDbConnectionConfig),
        Delete(String),
        Test(AeroricDbConnectionConfig),
        Connect(String),
    }
    if !request.params.is_object() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "requires params object",
        );
    }
    let operation = match request.method.as_str() {
        "database.dbx_list_connections" => Operation::List,
        "database.dbx_save_connection" | "database.dbx_test_connection" => {
            let connection =
                match serde_json::from_value(param_value(&request.params, "connection")) {
                    Ok(value) => value,
                    Err(error) => {
                        return failure(
                            request.id.clone(),
                            ErrorCode::InvalidArgument,
                            &format!("connection: {error}"),
                        )
                    }
                };
            if request.method == "database.dbx_save_connection" {
                Operation::Save(connection)
            } else {
                Operation::Test(connection)
            }
        }
        "database.dbx_delete_connection" | "database.dbx_connect" => {
            let Some(id) = str_param(&request.params, "connectionId") else {
                return invalid(&request, "connectionId (string)");
            };
            if request.method == "database.dbx_delete_connection" {
                Operation::Delete(id)
            } else {
                Operation::Connect(id)
            }
        }
        _ => {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                "unknown database dbx method",
            )
        }
    };
    // Check after queueing, not before: a request waiting behind another DBX
    // operation must not reuse an admission made against an earlier lease.
    let _operation = hosted.operations.lock().await;
    if let Some(refusal) = crate::file_access::write_refusal(&request, &context) {
        return refusal;
    }
    let state = hosted.state(&context).await;
    let paths = dbx_paths(&context);
    let store = dbx_store(&context);
    let result = match operation {
        Operation::List => {
            dbx::list_connections(state, &paths, &store)
                .await
                .and_then(|connections| {
                    serde_json::to_value(connections).map_err(|error| error.to_string())
                })
        }
        Operation::Save(connection) => dbx::save_connection(state, &paths, &store, connection)
            .await
            .map(|()| Value::Null),
        Operation::Delete(id) => dbx::delete_connection(state, &paths, &store, &id)
            .await
            .map(|()| Value::Null),
        Operation::Test(connection) => dbx::test_connection(state, &paths, &store, connection)
            .await
            .map(|()| Value::Null),
        Operation::Connect(id) => dbx::connect(state, &paths, &store, &id)
            .await
            .map(|()| Value::Null),
    };
    match result {
        Ok(value) => ok(&request, value),
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

// ── 共享形状 ────────────────────────────────────────────────────────────────

fn invalid(request: &Request, what: &str) -> Frame {
    failure(
        request.id.clone(),
        ErrorCode::InvalidArgument,
        &format!("requires params.{what}"),
    )
}

fn str_param(params: &Value, name: &str) -> Option<String> {
    params.get(name).and_then(Value::as_str).map(str::to_string)
}

fn int_param(params: &Value, name: &str) -> Option<i64> {
    params.get(name).and_then(Value::as_i64)
}

fn param_value(params: &Value, name: &str) -> Value {
    params.get(name).cloned().unwrap_or(Value::Null)
}

fn opt_string(request: &Request, params: &Value, name: &str) -> Result<Option<String>, Box<Frame>> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(Box::new(invalid(request, &format!("{name} (string)")))),
    }
}

/// endpoint 解码：`{"kind":"local","path":…}` 或 `{"kind":"ssh",…}`。
/// SSH 一律 `UnsupportedCapability`——宿主没有远程数据库执行器，下载/回传
/// 那半边留在 Tauri 适配层；这不是缺别名，是能力边界（与 takesTarget 的
/// 远程守卫同型）。
fn with_local(
    request: &Request,
    endpoint: Value,
    project_root: Option<String>,
    action: impl FnOnce(&Path) -> Result<Value, String> + Send + 'static,
) -> Frame {
    let raw_path = match endpoint.as_object().map(|object| {
        (
            object.get("kind").and_then(Value::as_str),
            object.get("path").and_then(Value::as_str),
        )
    }) {
        Some((Some("local"), Some(path))) => PathBuf::from(path),
        Some((Some("ssh"), _)) => {
            return failure(
                request.id.clone(),
                ErrorCode::UnsupportedCapability,
                "this host does not execute remote (ssh) database endpoints",
            );
        }
        Some((Some("local"), None)) => {
            return invalid(request, "endpoint.path (string)");
        }
        _ => {
            return invalid(request, "endpoint (object with kind local/ssh)");
        }
    };
    match biz::validate_local_db_path(&raw_path.to_string_lossy(), project_root.as_deref()) {
        Ok(path) => match action(&path) {
            Ok(value) => ok(request, value),
            Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
        },
        Err(error) => failure(request.id.clone(), ErrorCode::IoError, &error),
    }
}

// ── 连接文件（Value 直读直写：legacy 的 typed round-trip 会丢未知字段，
// 字节保真纪律 favor 保留——校验收紧到承重形状：数组＋id/name/endpoint.kind）──

fn load_connections(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!([]));
    }
    aeroric_core::settings::ensure_private_file_permissions(path)
        .map_err(|error| error.to_string())?;
    let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(&content).map_err(|error| error.to_string())?;
    if !value.is_array() {
        return Err("connections file must contain an array".to_string());
    }
    Ok(value)
}

fn validate_connections_shape(connections: &Value) -> Result<(), String> {
    let Some(entries) = connections.as_array() else {
        return Err("connections (array)".to_string());
    };
    for entry in entries {
        let Some(entry) = entry.as_object() else {
            return Err("connections (array of objects)".to_string());
        };
        let id_ok = entry
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty());
        let name_ok = entry
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty());
        let endpoint_ok = entry
            .get("endpoint")
            .and_then(Value::as_object)
            .is_some_and(|e| e.contains_key("kind"));
        if !id_ok || !name_ok || !endpoint_ok {
            return Err(
                "connections (each entry needs id, name and an endpoint object)".to_string(),
            );
        }
    }
    Ok(())
}

fn save_connections(path: &Path, connections: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(connections).map_err(|error| error.to_string())?;
    aeroric_core::settings::atomic_write_private(path, &format!("{content}\n"))
}

/// 权威只读判定（legacy `resolve_connection_read_only` 的 Value 版）。返回
/// `Err(文案)` 的两种情况：connection_id 指向的 saved 连接 endpoint 不匹配
/// （legacy 文案逐字），或生效只读却落在写命令上（调用方只对写命令取 Err；
/// 只读查询不受限）。找不到 saved 连接 ⇒ 用请求里的 readOnly。
fn effective_read_only(params: &Value, connections_path: &Path) -> Result<bool, String> {
    let requested = params
        .get("readOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let connection_id = params
        .get("connectionId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let content = match std::fs::read_to_string(connections_path) {
        Ok(content) => content,
        Err(_) => return Ok(requested),
    };
    let Ok(connections) = serde_json::from_str::<Value>(&content) else {
        return Ok(requested);
    };
    let Some(entries) = connections.as_array() else {
        return Ok(requested);
    };
    let endpoint = params.get("endpoint").cloned().unwrap_or(Value::Null);
    if let Some(id) = connection_id {
        if let Some(saved) = entries
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
        {
            if !endpoints_match_value(saved.get("endpoint").unwrap_or(&Value::Null), &endpoint) {
                return Err("Connection endpoint does not match saved configuration".to_string());
            }
            // 只读的拒绝发生在各写命令的 ensure_writable / execute_sql 的
            // 只读连接判定里——查询在只读连接上是合法的，这里只报事实。
            return Ok(saved
                .get("readOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false));
        }
    }
    let matching: Vec<&Value> = entries
        .iter()
        .filter(|entry| {
            endpoints_match_value(entry.get("endpoint").unwrap_or(&Value::Null), &endpoint)
        })
        .collect();
    if matching.is_empty() {
        return Ok(requested);
    }
    Ok(matching.iter().any(|entry| {
        entry
            .get("readOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }))
}

fn endpoints_match_value(left: &Value, right: &Value) -> bool {
    let (Some(left), Some(right)) = (left.as_object(), right.as_object()) else {
        return false;
    };
    let (Some(lk), Some(rk)) = (
        left.get("kind").and_then(Value::as_str),
        right.get("kind").and_then(Value::as_str),
    ) else {
        return false;
    };
    if lk != rk {
        return false;
    }
    match lk {
        "local" => {
            let lp = left.get("path").and_then(Value::as_str).unwrap_or_default();
            let rp = right
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let lc = std::fs::canonicalize(lp).unwrap_or(PathBuf::from(lp));
            let rc = std::fs::canonicalize(rp).unwrap_or(PathBuf::from(rp));
            lc == rc
        }
        "ssh" => {
            let path_match = left
                .get("path")
                .and_then(Value::as_str)
                .map(|p| p.trim_end_matches('/'))
                == right
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|p| p.trim_end_matches('/'));
            let connection_match = match (left.get("connection"), right.get("connection")) {
                (Some(l), Some(r)) => {
                    l.get("host").and_then(Value::as_str) == r.get("host").and_then(Value::as_str)
                        && l.get("port").and_then(Value::as_u64)
                            == r.get("port").and_then(Value::as_u64)
                        && l.get("username").and_then(Value::as_str)
                            == r.get("username").and_then(Value::as_str)
                }
                _ => false,
            };
            path_match && connection_match
        }
        _ => false,
    }
}
